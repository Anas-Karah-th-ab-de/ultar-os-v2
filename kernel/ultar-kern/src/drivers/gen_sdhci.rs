//! Ultar-Kern – Treiber „gen-sdhci“ (Treiber-Werkstatt).
//! Gerät: SD-/MMC-Kartenleser (PCI-Klasse 08/05) · Norm: SD Host Controller Simplified Specification 3.0
//! und SD Physical Layer Simplified Spec. Zielgerät: O2 Micro (1217:9863) im Beelink GTi15 Ultra.
//! Kein Linux-Code – eigene Umsetzung nach der offenen Norm.
//!
//! Umfang: Controller zurücksetzen und takten, Karte anmelden (CMD0/CMD8/ACMD41/CMD2/CMD3/CMD9/CMD7/
//! CMD16), Lesen über den Datenanschluss (PIO, ohne DMA). Ohne eingelegte Karte wird nur das
//! Hochfahren geprüft und kein Datenträger angemeldet. Schreiben folgt später.

use crate::driver::{BlockDev, Driver, Result, BLOCK};
use crate::pci::PciDev;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;

pub struct Drv;
pub static DRV: Drv = Drv;

// SDHCI-Register (Offsets ab BAR0)
const R_BLKSIZE: u64 = 0x04; // 16 Bit
const R_BLKCNT: u64 = 0x06; // 16 Bit
const R_ARG: u64 = 0x08;
const R_XFERMODE: u64 = 0x0c; // 16 Bit
const R_CMD: u64 = 0x0e; // 16 Bit
const R_RESP0: u64 = 0x10;
const R_BUFDATA: u64 = 0x20;
const R_PRESENT: u64 = 0x24;
const R_HOSTCTL1: u64 = 0x28; // 8 Bit
const R_PWRCTL: u64 = 0x29; // 8 Bit
const R_CLKCTL: u64 = 0x2c; // 16 Bit
const R_TIMEOUT: u64 = 0x2e; // 8 Bit
const R_SWRESET: u64 = 0x2f; // 8 Bit
const R_NORMINT: u64 = 0x30; // 16 Bit
const R_ERRINT: u64 = 0x32; // 16 Bit
const R_NORMINT_EN: u64 = 0x34; // 16 Bit
const R_ERRINT_EN: u64 = 0x36; // 16 Bit
const R_CAP: u64 = 0x40; // 64 Bit
const R_VERSION: u64 = 0xfe; // 16 Bit

// Present State
const PS_CMD_INHIBIT: u32 = 1 << 0;
const PS_DAT_INHIBIT: u32 = 1 << 1;
const PS_CARD_INSERTED: u32 = 1 << 16;

// Normal Interrupt Status
const NI_CMD_DONE: u16 = 1 << 0;
const NI_XFER_DONE: u16 = 1 << 1;
const NI_BUF_READ: u16 = 1 << 5;
const NI_ERROR: u16 = 1 << 15;

// Command-Register-Flags
const CMD_DATA: u16 = 1 << 5;
const CMD_IDX_CHECK: u16 = 1 << 4;
const CMD_CRC_CHECK: u16 = 1 << 3;
const RESP_NONE: u16 = 0;
const RESP_136: u16 = 1;
const RESP_48: u16 = 2;
const RESP_48_BUSY: u16 = 3;

// Transfer-Mode-Flags
const XFER_BLKCNT_EN: u16 = 1 << 1;
const XFER_AUTOCMD12: u16 = 1 << 2;
const XFER_READ: u16 = 1 << 4;
const XFER_MULTI: u16 = 1 << 5;

struct Mmio {
    base: u64,
}
impl Mmio {
    // SICHERHEIT: `base` ist der identisch und ungepuffert abgebildete BAR0 des Controllers; alle
    // Offsets liegen in dessen Registerfenster (< 0x100 bzw. 0xFE).
    fn r8(&self, o: u64) -> u8 {
        unsafe { core::ptr::read_volatile((self.base + o) as *const u8) }
    }
    fn r16(&self, o: u64) -> u16 {
        unsafe { core::ptr::read_volatile((self.base + o) as *const u16) }
    }
    fn r32(&self, o: u64) -> u32 {
        unsafe { core::ptr::read_volatile((self.base + o) as *const u32) }
    }
    fn w8(&self, o: u64, v: u8) {
        unsafe { core::ptr::write_volatile((self.base + o) as *mut u8, v) }
    }
    fn w16(&self, o: u64, v: u16) {
        unsafe { core::ptr::write_volatile((self.base + o) as *mut u16, v) }
    }
    fn w32(&self, o: u64, v: u32) {
        unsafe { core::ptr::write_volatile((self.base + o) as *mut u32, v) }
    }
}

fn wait(mut f: impl FnMut() -> bool, ms: u64) -> bool {
    let end = crate::arch::uptime_ms() + ms;
    while crate::arch::uptime_ms() < end {
        if f() {
            return true;
        }
        crate::arch::pause();
    }
    f()
}

struct Sd {
    name: String,
    m: Mmio,
    rca: u16,
    sdhc: bool, // Blockadressierung (SDHC/SDXC) statt Byteadressierung
    sectors: u64,
}

impl Sd {
    /// Ein Kommando absetzen und auf „Command Complete" warten. Gibt bei Fehler `Err`.
    fn cmd(&self, index: u8, arg: u32, resp: u16, data: bool) -> Result<()> {
        // warten, bis die Leitung frei ist
        if !wait(|| self.m.r32(R_PRESENT) & (PS_CMD_INHIBIT | if data { PS_DAT_INHIBIT } else { 0 }) == 0, 500) {
            return Err(format!("SD CMD{index}: Leitung belegt"));
        }
        self.m.w16(R_NORMINT, 0xffff); // alte Status-Bits löschen
        self.m.w16(R_ERRINT, 0xffff);
        self.m.w32(R_ARG, arg);
        let mut c = (index as u16) << 8 | resp;
        if resp == RESP_48 || resp == RESP_48_BUSY {
            c |= CMD_CRC_CHECK | CMD_IDX_CHECK;
        } else if resp == RESP_136 {
            c |= CMD_CRC_CHECK;
        }
        if data {
            c |= CMD_DATA;
        }
        self.m.w16(R_CMD, c);
        if !wait(|| self.m.r16(R_NORMINT) & (NI_CMD_DONE | NI_ERROR) != 0, 500) {
            return Err(format!("SD CMD{index}: keine Antwort"));
        }
        if self.m.r16(R_NORMINT) & NI_ERROR != 0 {
            let e = self.m.r16(R_ERRINT);
            return Err(format!("SD CMD{index}: Fehler {e:#06x}"));
        }
        Ok(())
    }

    fn resp0(&self) -> u32 {
        self.m.r32(R_RESP0)
    }
}

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "gen-sdhci"
    }
    fn origin(&self) -> &'static str {
        "werkstatt"
    }
    fn matches(&self, d: &PciDev) -> bool {
        // Klasse 08 (Base System Peripherals), Teilklasse 05 (SD Host Controller)
        d.class == 0x08 && d.subclass == 0x05
    }
    fn probe(&self, d: &PciDev) -> Result<()> {
        let bar = d.bars[0].filter(|b| !b.io).ok_or("BAR0 fehlt")?;
        d.enable();
        let m = Mmio { base: bar.addr };
        let ver = m.r16(R_VERSION) & 0xff;
        let cap = {
            let lo = m.r32(R_CAP) as u64;
            let hi = m.r32(R_CAP + 4) as u64;
            lo | hi << 32
        };
        // Basistakt (Bit 8..15 der Fähigkeiten, in MHz; 0 = unbekannt)
        let base_mhz = ((cap >> 8) & 0xff) as u32;

        // Vollständiger Reset
        m.w8(R_SWRESET, 0x01);
        if !wait(|| m.r8(R_SWRESET) & 0x01 == 0, 100) {
            return Err("SDHCI: Reset hängt".into());
        }
        // Timeout auf Maximum
        m.w8(R_TIMEOUT, 0x0e);
        // Status-Bits sammeln (wir fragen ab, keine Interrupt-Leitung)
        m.w16(R_NORMINT_EN, 0xffff);
        m.w16(R_ERRINT_EN, 0xffff);

        let inserted = m.r32(R_PRESENT) & PS_CARD_INSERTED != 0;
        if !inserted {
            crate::kprintln!("[sdhci] Controller {:04x}:{:04x} v{ver} (Basistakt {base_mhz} MHz) – keine Karte eingelegt", d.vendor, d.device);
            return Ok(());
        }

        // Takt für die Initialisierung (~400 kHz), dann Strom an
        set_clock(&m, base_mhz, 400)?;
        // 3,3 V (Bit 1..3 = 0b111) und einschalten (Bit 0)
        m.w8(R_PWRCTL, (0x7 << 1) | 1);
        crate::arch::sleep_us(2_000);

        let mut sd = Sd { name: String::new(), m, rca: 0, sdhc: false, sectors: 0 };
        init_card(&mut sd, base_mhz)?;

        let mut bl = BLOCK.lock();
        sd.name = format!("sd{}", bl.len());
        crate::kprintln!(
            "[sdhci] {} : SD{}-Karte {} MB (Controller v{ver}, {base_mhz} MHz)",
            sd.name,
            if sd.sdhc { "HC/XC" } else { "SC" },
            sd.sectors * 512 >> 20
        );
        bl.push(Box::new(sd));
        Ok(())
    }
}

/// SD-Takt setzen: Teiler aus Basistakt bestimmen, internen Takt an, warten bis stabil, SD-Takt an.
fn set_clock(m: &Mmio, base_mhz: u32, want_khz: u32) -> Result<()> {
    m.w16(R_CLKCTL, 0); // Takt aus
    let base_khz = if base_mhz == 0 { 50_000 } else { base_mhz * 1000 };
    // 10-Bit-Teiler (SDHCI 3.0): Taktfrequenz = Basis / (2*div); div 0 = Basis
    let mut div = 1u32;
    while base_khz / (2 * div) > want_khz && div < 0x3ff {
        div += 1;
    }
    let dsel = ((div & 0xff) << 8) | ((div & 0x300) >> 2);
    m.w16(R_CLKCTL, (dsel as u16) | 0x01); // interner Takt an
    if !wait(|| m.r16(R_CLKCTL) & 0x02 != 0, 100) {
        return Err("SDHCI: Takt nicht stabil".into());
    }
    m.w16(R_CLKCTL, m.r16(R_CLKCTL) | 0x04); // SD-Takt an
    Ok(())
}

/// SD-Karte anmelden: Leerlauf, Spannungsprüfung, Betriebsbedingungen, CID/RCA, Kapazität, Auswahl.
fn init_card(sd: &mut Sd, base_mhz: u32) -> Result<()> {
    sd.cmd(0, 0, RESP_NONE, false)?; // GO_IDLE_STATE
    // CMD8: Prüfmuster 0x1AA (Spannung 2,7–3,6 V). Fehlt die Antwort, ist es eine ältere Karte.
    let v2 = sd.cmd(8, 0x1aa, RESP_48, false).is_ok() && (sd.resp0() & 0xfff) == 0x1aa;
    // ACMD41-Schleife bis bereit (OCR Bit 31). HCS (Bit 30) nur bei v2.
    let hcs = if v2 { 1 << 30 } else { 0 };
    let mut ocr = 0u32;
    for _ in 0..100 {
        sd.cmd(55, 0, RESP_48, false)?; // APP_CMD (RCA 0)
        sd.cmd(41, hcs | 0x00ff_8000, RESP_48, false)?; // SD_SEND_OP_COND (3,3 V Bereich)
        ocr = sd.resp0();
        if ocr & (1 << 31) != 0 {
            break;
        }
        crate::arch::sleep_us(10_000);
    }
    if ocr & (1 << 31) == 0 {
        return Err("SD: Karte wird nicht bereit (ACMD41)".into());
    }
    sd.sdhc = ocr & (1 << 30) != 0; // CCS: Blockadressierung
    sd.cmd(2, 0, RESP_136, false)?; // ALL_SEND_CID
    sd.cmd(3, 0, RESP_48, false)?; // SEND_RELATIVE_ADDR → RCA in den oberen 16 Bit
    sd.rca = (sd.resp0() >> 16) as u16;
    sd.cmd(9, (sd.rca as u32) << 16, RESP_136, false)?; // SEND_CSD
    sd.sectors = csd_sectors(sd);
    if sd.sectors == 0 {
        return Err("SD: Kapazität aus CSD nicht lesbar".into());
    }
    // schnellerer Takt (25 MHz Normalgeschwindigkeit)
    set_clock(&sd.m, base_mhz, 25_000)?;
    sd.cmd(7, (sd.rca as u32) << 16, RESP_48_BUSY, false)?; // SELECT_CARD
    if !sd.sdhc {
        sd.cmd(16, 512, RESP_48, false)?; // SET_BLOCKLEN 512 (nur Byteadressierung)
    }
    Ok(())
}

/// Kapazität in 512-Byte-Sektoren aus dem 136-Bit-CSD. SDHCI legt die Antwort ohne CRC ab, also
/// stehen die CSD-Bits [127:8] in RESP[119:0] (RESP0 Bit 0 = CSD-Bit 8).
fn csd_sectors(sd: &Sd) -> u64 {
    let r0 = sd.m.r32(R_RESP0);
    let r1 = sd.m.r32(R_RESP0 + 4);
    let r2 = sd.m.r32(R_RESP0 + 8);
    let r3 = sd.m.r32(R_RESP0 + 12);
    // CSD-Version steht in CSD[127:126] = RESP[119:118] = (r3 >> 22) & 0b11
    let csd_ver = (r3 >> 22) & 0x3;
    if csd_ver == 1 {
        // v2 (SDHC/SDXC): C_SIZE = CSD[69:48] = RESP[61:40] = (r1 >> 8) & 0x3FFFFF
        let c_size = ((r1 >> 8) & 0x3f_ffff) as u64;
        (c_size + 1) * 1024 // jede Einheit = 512 KB = 1024 Sektoren
    } else {
        // v1 (SDSC): C_SIZE = CSD[73:62], C_SIZE_MULT = CSD[49:47], READ_BL_LEN = CSD[83:80]
        let read_bl_len = ((r2 >> 8) & 0xf) as u32; // RESP[83:80]
        let c_size = (((r2 & 0x3) << 10) | (r1 >> 22)) as u64; // RESP[73:62]
        let c_size_mult = ((r1 >> 7) & 0x7) as u32; // RESP[49:47]
        let blocks = (c_size + 1) * (1u64 << (c_size_mult + 2));
        let bl_bytes = 1u64 << read_bl_len;
        blocks * bl_bytes / 512
    }
}

impl BlockDev for Sd {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn sectors(&self) -> u64 {
        self.sectors
    }
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<()> {
        if buf.len() % 512 != 0 {
            return Err("Länge nicht durch 512 teilbar".into());
        }
        let count = buf.len() / 512;
        if count == 0 {
            return Ok(());
        }
        let addr = if self.sdhc { lba as u32 } else { (lba * 512) as u32 };
        self.m.w16(R_BLKSIZE, 512);
        self.m.w16(R_BLKCNT, count as u16);
        let mut mode = XFER_READ;
        let index = if count > 1 {
            mode |= XFER_MULTI | XFER_BLKCNT_EN | XFER_AUTOCMD12;
            18
        } else {
            17
        };
        self.m.w16(R_XFERMODE, mode);
        // CMD17/18 mit Datenphase
        self.cmd(index, addr, RESP_48, true)?;
        for blk in 0..count {
            if !wait(|| self.m.r16(R_NORMINT) & (NI_BUF_READ | NI_ERROR) != 0, 1000) {
                return Err("SD: Lesepuffer nicht bereit".into());
            }
            if self.m.r16(R_NORMINT) & NI_ERROR != 0 {
                return Err(format!("SD: Lesefehler {:#06x}", self.m.r16(R_ERRINT)));
            }
            self.m.w16(R_NORMINT, NI_BUF_READ);
            for i in 0..128 {
                let w = self.m.r32(R_BUFDATA);
                let off = blk * 512 + i * 4;
                buf[off..off + 4].copy_from_slice(&w.to_le_bytes());
            }
        }
        if !wait(|| self.m.r16(R_NORMINT) & (NI_XFER_DONE | NI_ERROR) != 0, 1000) {
            return Err("SD: Übertragung nicht abgeschlossen".into());
        }
        self.m.w16(R_NORMINT, NI_XFER_DONE);
        Ok(())
    }
}
