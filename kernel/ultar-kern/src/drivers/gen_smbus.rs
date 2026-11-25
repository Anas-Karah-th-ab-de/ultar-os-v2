//! Ultar-Kern – Treiber „gen-smbus“ (Treiber-Werkstatt).
//! Gerät: Intel-SMBus-Controller der ICH/PCH-Chipsätze (8086, Klasse 0c.05) · Norm: SMBus 2.0/3.0 und die öffentlichen
//! Intel-Datenblätter (ICH9 „SMBus Controller Registers“, D31:F3): E/A-Register HST_STS/HST_CNT/HST_CMD/XMIT_SLVA/HST_D0,
//! Konfigurationsregister HOSTC (0x40). Umfang: Controller einschalten, „Byte lesen“ im Abfragebetrieb,
//! Speicherriegel-Kennungen (SPD, Adressen 0x50–0x57) erfassen. Schnittstelle: `lies()`, `geraete()`.
//! Auftrag: werkstatt/auftraege/smbus.md · Status: ausgeschrieben, in QEMU (q35/ICH9) geprüft.

use crate::arch::{inb, outb};
use crate::driver::{Driver, Result};
use crate::pci::PciDev;
use alloc::vec::Vec;
use spin::Mutex;

pub struct Drv;
pub static DRV: Drv = Drv;

const STS: u16 = 0;
const CNT: u16 = 2;
const CMD: u16 = 3;
const ADR: u16 = 4;
const D0: u16 = 5;

/// (E/A-Basis, gefundene Geräte: Adresse und SPD-Speichertyp)
static BUS: Mutex<Option<(u16, Vec<(u8, u8)>)>> = Mutex::new(None);

fn wait(mut f: impl FnMut() -> bool, ms: u64) -> bool {
    let end = crate::arch::uptime_ms() + ms;
    while crate::arch::uptime_ms() <= end {
        if f() {
            return true;
        }
        crate::arch::pause();
    }
    f()
}

/// „Read Byte“: Register `reg` des Geräts an Adresse `adr` (7 Bit)
fn lies_byte(b: u16, adr: u8, reg: u8) -> Option<u8> {
    unsafe {
        if !wait(|| inb(b + STS) & 1 == 0, 20) {
            outb(b + CNT, 0x02); // laufende Übertragung abbrechen (KILL)
            outb(b + CNT, 0);
        }
        outb(b + STS, 0xff);
        outb(b + ADR, adr << 1 | 1);
        outb(b + CMD, reg);
        outb(b + CNT, 0x48); // START + Protokoll „Byte Data“
        let mut st = 0;
        let fertig = wait(
            || {
                st = inb(b + STS);
                st & 1 == 0 && st & 0x1e != 0
            },
            30,
        );
        let v = inb(b + D0);
        outb(b + STS, 0xff);
        if !fertig || st & 0x1c != 0 {
            return None;
        }
        Some(v)
    }
}

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "gen-smbus"
    }
    fn origin(&self) -> &'static str {
        "werkstatt"
    }
    fn matches(&self, d: &PciDev) -> bool {
        d.vendor == 0x8086 && d.class == 0x0c && d.subclass == 0x05
    }
    fn probe(&self, d: &PciDev) -> Result<()> {
        let bar = d.bars[4].filter(|b| b.io).ok_or("E/A-Bereich (BAR4) fehlt")?;
        // E/A-Zugriff im Befehlsregister an, Unterbrechungen aus; HOSTC: Controller ein, kein SMI, kein I2C-Modus
        let cmd = d.r32(0x04) & 0xffff;
        d.w32(0x04, cmd | 0x0401);
        let hostc = d.r32(0x40);
        d.w32(0x40, (hostc & !0x06) | 0x01);
        let b = bar.addr as u16;
        let mut funde = Vec::new();
        for adr in 0x50..=0x57u8 {
            if let Some(typ) = lies_byte(b, adr, 2) {
                funde.push((adr, typ));
            }
        }
        crate::kprintln!("[smbus] Controller E/A {b:#x}: {} Gerät(e) an 0x50–0x57 antworten (Speicherriegel-Kennungen)", funde.len());
        for (adr, typ) in funde.iter().filter(|x| x.1 != 0 && x.1 != 0xff) {
            crate::kprintln!("[smbus]   {adr:#x}: {}", speichertyp(*typ));
        }
        *BUS.lock() = Some((b, funde));
        Ok(())
    }
}

/// SPD-Byte 2 (JEDEC): Speichertyp
pub fn speichertyp(t: u8) -> &'static str {
    match t {
        0x04 => "SDRAM",
        0x07 => "DDR",
        0x08 => "DDR2",
        0x0b => "DDR3",
        0x0c => "DDR4",
        0x12 => "DDR5",
        0x00 | 0xff => "leer",
        _ => "unbekannter Typ",
    }
}

/// Byte von einem SMBus-Gerät lesen (für Konsole und spätere Sensor-Treiber)
pub fn lies(adr: u8, reg: u8) -> Option<u8> {
    let b = BUS.lock().as_ref()?.0;
    lies_byte(b, adr & 0x7f, reg)
}

/// beim Start gefundene Geräte: (Adresse, SPD-Speichertyp)
pub fn geraete() -> Option<Vec<(u8, u8)>> {
    BUS.lock().as_ref().map(|x| x.1.clone())
}
