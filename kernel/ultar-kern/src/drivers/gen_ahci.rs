//! Ultar-Kern – Treiber „gen-ahci“ (Treiber-Werkstatt).
//! Gerät: SATA (AHCI) 8086:2922 (Klasse 010601) · Norm: Serial ATA AHCI 1.3.1 (Intel)
//! Auftrag: werkstatt/auftraege/ahci.md · Status: ausgeschrieben aus der Norm (Abschnitte 3, 4, 5, 10.1),
//! ohne Linux-Code. SATA-Festplatten/SSDs an jedem AHCI-Controller (PCs, Laptops, Server).

use crate::driver::{BlockDev, Driver, Result, BLOCK};
use crate::mem::alloc_pages;
use crate::pci::PciDev;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use core::ptr::{read_volatile, write_volatile};

pub struct Drv;
pub static DRV: Drv = Drv;

// Register des Controllers (HBA) und je Port (Norm Abschnitt 3.1 / 3.3)
const CAP: u64 = 0x00;
const GHC: u64 = 0x04;
const PI: u64 = 0x0c;
const P_CLB: u64 = 0x00;
const P_FB: u64 = 0x08;
const P_IS: u64 = 0x10;
const P_IE: u64 = 0x14;
const P_CMD: u64 = 0x18;
const P_TFD: u64 = 0x20;
const P_SIG: u64 = 0x24;
const P_SSTS: u64 = 0x28;
const P_SERR: u64 = 0x30;
const P_CI: u64 = 0x38;

const CMD_ST: u32 = 1 << 0;
const CMD_FRE: u32 = 1 << 4;
const CMD_FR: u32 = 1 << 14;
const CMD_CR: u32 = 1 << 15;
const IS_TFES: u32 = 1 << 30;

const ATA_IDENTIFY: u8 = 0xec;
const ATA_READ_DMA_EXT: u8 = 0x25;
const ATA_WRITE_DMA_EXT: u8 = 0x35;
/// FLUSH CACHE EXT (ACS-4 §7.11), no data
const ATA_FLUSH_CACHE_EXT: u8 = 0xea;
/// pro Befehl höchstens 64 KB (eine PRD-Zeile je 4-KB-Seite, 16 Zeilen)
const MAXB: usize = 64 * 1024;

unsafe fn r(a: u64) -> u32 {
    read_volatile(a as *const u32)
}
unsafe fn w(a: u64, v: u32) {
    write_volatile(a as *mut u32, v)
}

fn wait(cond: impl Fn() -> bool, ms: u64) -> bool {
    let end = crate::arch::uptime_ms() + ms;
    while !cond() {
        if crate::arch::uptime_ms() > end {
            return false;
        }
        crate::arch::pause();
    }
    true
}

struct Disk {
    name: String,
    port: u64,
    /// Befehlsliste (1 KB), FIS-Empfang (256 B), Befehlstabelle (Slot 0), Datenpuffer
    clb: u64,
    ctba: u64,
    data: u64,
    sectors: u64,
}

impl Disk {
    /// ATA-Befehl über Slot 0 ausführen; `bytes` Daten vom Gerät nach `self.data`
    fn ata(&mut self, cmd: u8, lba: u64, count: u16, bytes: usize) -> Result<()> {
        let p = self.port;
        unsafe {
            if !wait(|| r(p + P_TFD) & 0x88 == 0, 1000) {
                return Err("Laufwerk beschäftigt".into());
            }
            // Befehlstabelle: H2D-Register-FIS (Norm Abschnitt 4.2.3, SATA 10.5.6)
            core::ptr::write_bytes(self.ctba as *mut u8, 0, 0x80 + 16 * 16);
            let f = self.ctba as *mut u8;
            *f = 0x27;
            *f.add(1) = 0x80;
            *f.add(2) = cmd;
            *f.add(4) = lba as u8;
            *f.add(5) = (lba >> 8) as u8;
            *f.add(6) = (lba >> 16) as u8;
            *f.add(7) = 1 << 6;
            *f.add(8) = (lba >> 24) as u8;
            *f.add(9) = (lba >> 32) as u8;
            *f.add(10) = (lba >> 40) as u8;
            *f.add(12) = count as u8;
            *f.add(13) = (count >> 8) as u8;
            // PRD-Tabelle: eine Zeile je 4-KB-Seite
            let pages = bytes.div_ceil(4096).max(1);
            for i in 0..pages {
                let e = self.ctba + 0x80 + i as u64 * 16;
                let a = self.data + i as u64 * 4096;
                let n = (bytes - i * 4096).min(4096);
                w(e, a as u32);
                w(e + 4, (a >> 32) as u32);
                w(e + 12, (n as u32 - 1) & 0x3f_ffff);
            }
            // Kopf in der Befehlsliste: FIS 5 Doppelwörter, W-Bit beim Schreiben, PRDTL
            w(self.clb, 5 | if cmd == ATA_WRITE_DMA_EXT { 1 << 6 } else { 0 } | ((pages as u32) << 16));
            w(self.clb + 4, 0);
            w(self.clb + 8, self.ctba as u32);
            w(self.clb + 12, (self.ctba >> 32) as u32);
            w(p + P_IS, 0xffff_ffff);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            w(p + P_CI, 1);
            let done = wait(|| r(p + P_CI) & 1 == 0 || r(p + P_IS) & IS_TFES != 0, 5000);
            let tfd = r(p + P_TFD);
            if !done {
                return Err("Zeitüberschreitung".into());
            }
            if r(p + P_IS) & IS_TFES != 0 || tfd & 1 != 0 {
                return Err(format!("ATA-Fehler (TFD {:#x})", tfd));
            }
        }
        Ok(())
    }
}

/// Port anhalten, Speicher zuweisen, wieder starten (Norm Abschnitt 10.1.2 / 10.3)
unsafe fn port_init(p: u64) -> Option<(u64, u64, u64)> {
    w(p + P_CMD, r(p + P_CMD) & !CMD_ST);
    if !wait(|| r(p + P_CMD) & CMD_CR == 0, 500) {
        return None;
    }
    w(p + P_CMD, r(p + P_CMD) & !CMD_FRE);
    if !wait(|| r(p + P_CMD) & CMD_FR == 0, 500) {
        return None;
    }
    let mem = alloc_pages(1)?; // 0x000 Befehlsliste (1 KB), 0x400 FIS-Empfang (256 B)
    let ctba = alloc_pages(1)?;
    let data = alloc_pages(MAXB / 4096)?;
    w(p + P_CLB, mem as u32);
    w(p + P_CLB + 4, (mem >> 32) as u32);
    w(p + P_FB, (mem + 0x400) as u32);
    w(p + P_FB + 4, ((mem + 0x400) >> 32) as u32);
    w(p + P_SERR, 0xffff_ffff);
    w(p + P_IS, 0xffff_ffff);
    w(p + P_IE, 0); // Abfragebetrieb
    w(p + P_CMD, r(p + P_CMD) | CMD_FRE);
    wait(|| r(p + P_TFD) & 0x88 == 0, 1000);
    w(p + P_CMD, r(p + P_CMD) | CMD_ST);
    Some((mem, ctba, data))
}

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "gen-ahci"
    }
    fn origin(&self) -> &'static str {
        "werkstatt"
    }
    fn matches(&self, d: &PciDev) -> bool {
        d.class == 0x01 && d.subclass == 0x06 && d.prog_if == 0x01
    }
    fn probe(&self, d: &PciDev) -> Result<()> {
        d.enable();
        let abar = d.bars[5].filter(|b| !b.io).ok_or("ABAR (BAR5) fehlt")?.addr;
        unsafe {
            // Controller zurücksetzen (GHC.HR), dann AHCI-Modus (Norm 10.4.3)
            w(abar + GHC, r(abar + GHC) | (1 << 31));
            w(abar + GHC, r(abar + GHC) | 1);
            if !wait(|| r(abar + GHC) & 1 == 0, 1000) {
                return Err("Controller-Reset hängt".into());
            }
            w(abar + GHC, r(abar + GHC) | (1 << 31));
            let pi = r(abar + PI);
            let nports = (r(abar + CAP) & 0x1f) + 1;
            let mut found = 0;
            for i in 0..32u64 {
                if pi & (1 << i) == 0 {
                    continue;
                }
                let p = abar + 0x100 + i * 0x80;
                // Laufwerk hochfahren (SUD) und auf die Verbindung warten (DET = 3)
                w(p + P_CMD, r(p + P_CMD) | (1 << 1) | (1 << 2));
                // leere Ports schnell überspringen: ohne Gerät bleibt DET 0
                if !wait(|| r(p + P_SSTS) & 0xf != 0, 15) || !wait(|| r(p + P_SSTS) & 0xf == 3, 200) {
                    continue;
                }
                let Some((clb, ctba, data)) = port_init(p) else {
                    crate::kprintln!("[ahci] Port {i}: lässt sich nicht anhalten");
                    continue;
                };
                // Signatur kommt mit dem ersten FIS vom Gerät (erst nach FRE gültig)
                wait(|| r(p + P_SIG) != 0xffff_ffff, 500);
                if r(p + P_SIG) != 0x0000_0101 {
                    let sig = r(p + P_SIG);
                    let what = if sig == 0xeb14_0101 { "CD/DVD-Laufwerk (ATAPI) – Treiber folgt" } else { "unbekanntes Gerät" };
                    crate::kprintln!("[ahci] Port {i}: {what} (Signatur {sig:#x})");
                    continue;
                }
                let mut disk = Disk { name: String::new(), port: p, clb, ctba, data, sectors: 0 };
                if let Err(e) = disk.ata(ATA_IDENTIFY, 0, 0, 512) {
                    crate::kprintln!("[ahci] Port {i}: IDENTIFY {e}");
                    continue;
                }
                let id = core::slice::from_raw_parts(data as *const u16, 256);
                disk.sectors = id[100] as u64 | (id[101] as u64) << 16 | (id[102] as u64) << 32 | (id[103] as u64) << 48;
                if disk.sectors == 0 {
                    disk.sectors = id[60] as u64 | (id[61] as u64) << 16;
                }
                // Modellname: Wörter 27–46, je Wort Bytes vertauscht
                let mut model = String::new();
                for wd in &id[27..47] {
                    model.push((wd >> 8) as u8 as char);
                    model.push(*wd as u8 as char);
                }
                let mut bl = BLOCK.lock();
                disk.name = format!("sata{}", bl.iter().filter(|b| b.name().starts_with("sata")).count());
                crate::kprintln!("[ahci] {}: {}, {} MB (Port {i} von {nports})", disk.name, model.trim(), disk.sectors / 2048);
                bl.push(Box::new(disk));
                found += 1;
            }
            if found == 0 {
                crate::kprintln!("[ahci] Controller {}: keine SATA-Laufwerke angeschlossen", d.id());
            }
        }
        Ok(())
    }
}

impl BlockDev for Disk {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn sectors(&self) -> u64 {
        self.sectors
    }
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<()> {
        for (i, chunk) in buf.chunks_mut(MAXB).enumerate() {
            let start = lba + (i * MAXB / 512) as u64;
            self.ata(ATA_READ_DMA_EXT, start, (chunk.len() / 512) as u16, chunk.len())?;
            unsafe { core::ptr::copy_nonoverlapping(self.data as *const u8, chunk.as_mut_ptr(), chunk.len()) };
        }
        Ok(())
    }
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<()> {
        for (i, chunk) in buf.chunks(MAXB).enumerate() {
            let start = lba + (i * MAXB / 512) as u64;
            unsafe { core::ptr::copy_nonoverlapping(chunk.as_ptr(), self.data as *mut u8, chunk.len()) };
            self.ata(ATA_WRITE_DMA_EXT, start, (chunk.len() / 512) as u16, chunk.len())?;
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<()> {
        // a non-data command: the one PRD entry `ata` always fills is not used by the device
        self.ata(ATA_FLUSH_CACHE_EXT, 0, 0, 512)
    }
}
