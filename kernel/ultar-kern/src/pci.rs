//! PCI/PCIe: Geräte über den Konfigurationsraum (Ports 0xCF8/0xCFC) aufzählen, BARs bestimmen,
//! Bus-Master einschalten, Fähigkeitslisten lesen.
//!
//! PCIe-Erweiterung (ECAM): die ersten 256 Byte gehen weiter über die Ports; der erweiterte Raum
//! 0x100–0xFFF (DVSEC, AER, PTM, Resizable BAR …) über das speicherabgebildete Fenster, das die
//! ACPI-Tabelle MCFG nennt – `PciDev::r32x/w32x/ext_caps`. Ohne MCFG (alte Rechner) liefern sie `None`.

use crate::arch::{inl, outl};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

/// ECAM-Fenster von Segment 0: Basis (Adresse von Bus 0), erster und letzter Bus (Bits 0–7 / 8–15).
static ECAM_BASE: AtomicU64 = AtomicU64::new(0);
static ECAM_BUSES: AtomicU64 = AtomicU64::new(0);

/// Liest die ACPI-Tabelle MCFG und bildet das ECAM-Fenster von Segment 0 ab (Gerätespeicher,
/// ohne Zwischenspeicher). Vor `scan()` aufrufen. Gibt die Basis zurück (0 = kein ECAM).
pub fn init_ecam(rsdp: u64) -> u64 {
    // SICHERHEIT: find_table liest nur die von der Firmware übergebenen ACPI-Tabellen, die im
    // 1:1-abgebildeten Speicher liegen; MCFG-Einträge werden nur innerhalb der Tabellenlänge gelesen.
    let Some(t) = (unsafe { crate::apic::find_table(rsdp, b"MCFG") }) else {
        return 0;
    };
    let len = unsafe { core::ptr::read_unaligned((t + 4) as *const u32) } as u64;
    let mut off = 44;
    while off + 16 <= len {
        // Eintrag: Basis u64, Segment u16, erster Bus u8, letzter Bus u8, reserviert u32
        let base = unsafe { core::ptr::read_unaligned((t + off) as *const u64) };
        let seg = unsafe { core::ptr::read_unaligned((t + off + 8) as *const u16) };
        let (first, last) = unsafe { (*((t + off + 10) as *const u8), *((t + off + 11) as *const u8)) };
        off += 16;
        if seg != 0 || base == 0 || last < first || base & 0xf_ffff != 0 {
            continue;
        }
        let start = base + ((first as u64) << 20);
        let end = base + ((last as u64 + 1) << 20);
        crate::paging::map_identity(start, end, true);
        ECAM_BUSES.store(first as u64 | (last as u64) << 8, Ordering::Relaxed);
        ECAM_BASE.store(base, Ordering::Release);
        return base;
    }
    0
}

/// Adresse eines Registers im ECAM-Fenster, wenn Bus und Offset darin liegen.
fn ecam_addr(b: u8, d: u8, f: u8, off: u16) -> Option<u64> {
    let base = ECAM_BASE.load(Ordering::Acquire);
    let buses = ECAM_BUSES.load(Ordering::Relaxed);
    let (first, last) = ((buses & 0xff) as u8, (buses >> 8 & 0xff) as u8);
    if base == 0 || b < first || b > last || d > 31 || f > 7 || off > 0xffc {
        return None;
    }
    Some(base + ((b as u64) << 20 | (d as u64) << 15 | (f as u64) << 12 | (off as u64 & 0xffc)))
}

/// Erweiterter Konfigurationsraum lesen (0x000–0xFFF), `None` ohne ECAM.
pub fn read32x(b: u8, d: u8, f: u8, off: u16) -> Option<u32> {
    // SICHERHEIT: ecam_addr liefert nur Adressen im von init_ecam abgebildeten ECAM-Fenster
    // (Gerätespeicher, PCD/PWT); 32-Bit-Zugriff auf 4-Byte-ausgerichtete Register, volatil.
    ecam_addr(b, d, f, off).map(|a| unsafe { core::ptr::read_volatile(a as *const u32) })
}

/// Erweiterter Konfigurationsraum schreiben; `false` ohne ECAM.
#[allow(dead_code)] // nothing uses it yet; kept, see docs/OFFEN-WARNUNGEN.md
pub fn write32x(b: u8, d: u8, f: u8, off: u16, v: u32) -> bool {
    match ecam_addr(b, d, f, off) {
        Some(a) => {
            // SICHERHEIT: wie read32x – nur im abgebildeten ECAM-Fenster, ausgerichtet, volatil.
            unsafe { core::ptr::write_volatile(a as *mut u32, v) };
            true
        }
        None => false,
    }
}

/// Ist der erweiterte Konfigurationsraum (ECAM) verfügbar?
pub fn ecam_available() -> bool {
    ECAM_BASE.load(Ordering::Acquire) != 0
}

#[derive(Clone, Copy, Debug)]
pub struct Bar {
    pub addr: u64,
    pub size: u64,
    pub io: bool,
}

#[derive(Clone, Debug)]
pub struct PciDev {
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
    pub vendor: u16,
    pub device: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub rev: u8,
    pub subsys_vendor: u16,
    pub subsys: u16,
    pub bars: [Option<Bar>; 6],
    /// Name des Treibers, der das Gerät übernommen hat
    #[allow(dead_code)] // nothing uses it yet; kept, see docs/OFFEN-WARNUNGEN.md
    pub driver: Option<&'static str>,
}

fn addr(b: u8, d: u8, f: u8, off: u8) -> u32 {
    0x8000_0000 | (b as u32) << 16 | (d as u32) << 11 | (f as u32) << 8 | (off as u32 & 0xfc)
}

pub fn read32(b: u8, d: u8, f: u8, off: u8) -> u32 {
    unsafe {
        outl(0xcf8, addr(b, d, f, off));
        inl(0xcfc)
    }
}
pub fn write32(b: u8, d: u8, f: u8, off: u8, v: u32) {
    unsafe {
        outl(0xcf8, addr(b, d, f, off));
        outl(0xcfc, v);
    }
}
pub fn read16(b: u8, d: u8, f: u8, off: u8) -> u16 {
    (read32(b, d, f, off) >> ((off & 2) * 8)) as u16
}
pub fn read8(b: u8, d: u8, f: u8, off: u8) -> u8 {
    (read32(b, d, f, off) >> ((off & 3) * 8)) as u8
}

impl PciDev {
    pub fn r32(&self, off: u8) -> u32 {
        read32(self.bus, self.dev, self.func, off)
    }
    pub fn w32(&self, off: u8, v: u32) {
        write32(self.bus, self.dev, self.func, off, v)
    }
    pub fn r8(&self, off: u8) -> u8 {
        read8(self.bus, self.dev, self.func, off)
    }
    pub fn r16(&self, off: u8) -> u16 {
        read16(self.bus, self.dev, self.func, off)
    }
    /// Erweiterter Konfigurationsraum (0x000–0xFFF, PCIe/ECAM); `None` ohne ECAM.
    pub fn r32x(&self, off: u16) -> Option<u32> {
        read32x(self.bus, self.dev, self.func, off)
    }
    /// Erweiterter Konfigurationsraum schreiben; `false` ohne ECAM.
    #[allow(dead_code)] // nothing uses it yet; kept, see docs/OFFEN-WARNUNGEN.md
    pub fn w32x(&self, off: u16, v: u32) -> bool {
        write32x(self.bus, self.dev, self.func, off, v)
    }
    /// Erweiterte Fähigkeiten ab 0x100: (Position, Kennung, Version) – z. B. 0x23 = DVSEC,
    /// 0x01 = AER, 0x15 = Resizable BAR. Leer ohne ECAM oder ohne erweiterte Fähigkeiten.
    pub fn ext_caps(&self) -> Vec<(u16, u16, u8)> {
        let mut v = Vec::new();
        let mut p: u16 = 0x100;
        for _ in 0..64 {
            let Some(h) = self.r32x(p) else { break };
            if h == 0 || h == 0xffff_ffff {
                break;
            }
            v.push((p, (h & 0xffff) as u16, ((h >> 16) & 0xf) as u8));
            let next = ((h >> 20) & 0xffc) as u16;
            if next < 0x100 || next <= p {
                break;
            }
            p = next;
        }
        v
    }
    /// Speicherzugriff + Bus-Master (DMA) an, alte INTx-Interrupts aus (wir fragen ab)
    pub fn enable(&self) {
        let cmd = self.r32(0x04);
        self.w32(0x04, (cmd & 0xffff_0000) | (cmd & 0xffff) | 0x0006 | 0x0400);
    }
    /// Fähigkeitsliste: (Position, Kennung)
    pub fn caps(&self) -> Vec<(u8, u8)> {
        let mut v = Vec::new();
        if self.r32(0x04) >> 16 & 0x10 == 0 {
            return v;
        }
        let mut p = self.r8(0x34) & 0xfc;
        let mut guard = 0;
        while p != 0 && guard < 48 {
            v.push((p, self.r8(p)));
            p = self.r8(p + 1) & 0xfc;
            guard += 1;
        }
        v
    }
    pub fn id(&self) -> alloc::string::String {
        alloc::format!("{:02x}:{:02x}.{} {:04x}:{:04x}", self.bus, self.dev, self.func, self.vendor, self.device)
    }
    pub fn class_name(&self) -> &'static str {
        class_name(self.class, self.subclass, self.prog_if)
    }
}

fn probe_bars(b: u8, d: u8, f: u8) -> [Option<Bar>; 6] {
    let mut bars = [None; 6];
    let mut i = 0;
    while i < 6 {
        let off = 0x10 + i as u8 * 4;
        let v = read32(b, d, f, off);
        if v == 0 {
            i += 1;
            continue;
        }
        if v & 1 == 1 {
            write32(b, d, f, off, 0xffff_ffff);
            let s = read32(b, d, f, off);
            write32(b, d, f, off, v);
            let size = (!(s & 0xffff_fffc)).wrapping_add(1) & 0xffff;
            bars[i] = Some(Bar { addr: (v & 0xffff_fffc) as u64, size: size as u64, io: true });
            i += 1;
        } else {
            let is64 = (v >> 1) & 3 == 2;
            write32(b, d, f, off, 0xffff_ffff);
            let s = read32(b, d, f, off);
            write32(b, d, f, off, v);
            let mut base = (v & 0xffff_fff0) as u64;
            let mut mask = (s & 0xffff_fff0) as u64 | 0xffff_ffff_0000_0000;
            if is64 {
                let hi = read32(b, d, f, off + 4);
                write32(b, d, f, off + 4, 0xffff_ffff);
                let sh = read32(b, d, f, off + 4);
                write32(b, d, f, off + 4, hi);
                base |= (hi as u64) << 32;
                mask = ((sh as u64) << 32) | (s & 0xffff_fff0) as u64;
            }
            let size = (!mask).wrapping_add(1);
            if base != 0 && size != 0 && size < 1 << 40 {
                crate::paging::map_identity(base, base + size, true);
            }
            bars[i] = Some(Bar { addr: base, size, io: false });
            i += if is64 { 2 } else { 1 };
        }
    }
    bars
}

pub fn scan() -> Vec<PciDev> {
    let mut out = Vec::new();
    for b in 0..=255u8 {
        for d in 0..32u8 {
            let hdr = read8(b, d, 0, 0x0e);
            let funcs = if hdr & 0x80 != 0 { 8 } else { 1 };
            for f in 0..funcs {
                let id = read32(b, d, f, 0);
                if id & 0xffff == 0xffff {
                    if f == 0 {
                        break;
                    }
                    continue;
                }
                let cls = read32(b, d, f, 0x08);
                let htype = read8(b, d, f, 0x0e) & 0x7f;
                let sub = read32(b, d, f, 0x2c);
                out.push(PciDev {
                    bus: b,
                    dev: d,
                    func: f,
                    vendor: id as u16,
                    device: (id >> 16) as u16,
                    class: (cls >> 24) as u8,
                    subclass: (cls >> 16) as u8,
                    prog_if: (cls >> 8) as u8,
                    rev: cls as u8,
                    subsys_vendor: if htype == 0 { sub as u16 } else { 0 },
                    subsys: if htype == 0 { (sub >> 16) as u16 } else { 0 },
                    bars: if htype == 0 {
                        // während der Größenbestimmung Dekodierung aus (sonst kurzzeitig falsche Adressen am Bus)
                        let cmd = read32(b, d, f, 0x04);
                        write32(b, d, f, 0x04, cmd & !0x3);
                        let r = probe_bars(b, d, f);
                        write32(b, d, f, 0x04, cmd);
                        r
                    } else {
                        [None; 6]
                    },
                    driver: None,
                });
            }
        }
    }
    out
}

/// Geräteklasse laut PCI-SIG (Auszug, für Anzeige und Werkstatt)
pub fn class_name(c: u8, s: u8, p: u8) -> &'static str {
    match (c, s, p) {
        (0x01, 0x01, _) => "IDE-Controller",
        (0x01, 0x06, 0x01) => "SATA (AHCI)",
        (0x01, 0x08, 0x02) => "NVMe-Speicher",
        (0x01, 0x00, _) => "SCSI-Speicher",
        (0x01, _, _) => "Massenspeicher",
        (0x02, 0x00, _) => "Ethernet",
        (0x02, 0x80, _) => "Netzwerk (WLAN u. a.)",
        (0x02, _, _) => "Netzwerk",
        (0x03, 0x00, _) => "Grafik (VGA)",
        (0x03, _, _) => "Grafik",
        (0x04, 0x03, _) => "Audio (HD Audio)",
        (0x04, _, _) => "Multimedia",
        (0x06, 0x00, _) => "Host-Brücke",
        (0x06, 0x01, _) => "ISA-Brücke",
        (0x06, 0x04, _) => "PCI-Brücke",
        (0x06, _, _) => "Brücke",
        (0x07, _, _) => "Kommunikation (seriell)",
        (0x08, _, _) => "Systemperipherie",
        (0x09, _, _) => "Eingabe",
        (0x0c, 0x03, 0x30) => "USB 3 (xHCI)",
        (0x0c, 0x03, 0x20) => "USB 2 (EHCI)",
        (0x0c, 0x03, _) => "USB (alt)",
        (0x0c, 0x05, _) => "SMBus",
        (0x0c, _, _) => "Serieller Bus",
        (0x0d, _, _) => "Funk (Bluetooth u. a.)",
        (0x10, _, _) => "Verschlüsselung",
        (0x11, _, _) => "Signalverarbeitung",
        (0x12, _, _) => "Rechenbeschleuniger (NPU)",
        _ => "unbekannt",
    }
}
