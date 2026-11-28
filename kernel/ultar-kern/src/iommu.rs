//! IOMMU (Intel VT-d): DMA-Umleitung. Ohne sie darf jedes Gerät per Bus-Master in jeden physischen
//! Speicher schreiben – der klassische Angriff eines bösartigen oder fehlerhaften Geräts (auch über
//! Modem-/Chip-Firmware). Mit VT-d übersetzt eine Hardware-Einheit jede Geräte-Adresse über
//! Seitentabellen; was dort nicht abgebildet ist, löst einen Fehler aus statt eines Zugriffs.
//!
//! Stufe 1 (hier): eine gemeinsame zweite Übersetzungsebene bildet **nur den nutzbaren Arbeits-
//! speicher** 1:1 ab und sonst nichts. Damit laufen alle Treiber unverändert weiter (ihre DMA-Puffer
//! liegen im RAM), aber kein Gerät erreicht mehr Firmware, SMRAM, MMIO-Register anderer Geräte oder
//! Speicherlöcher. Jedes Gerät bekommt denselben Bereich (Standard-Domäne). Eine eigene Domäne je
//! Gerät mit nur seinen Puffern ist der nächste Schritt (docs/integration/iommu.md).
//!
//! Unterbrechungs-Umleitung bleibt aus: die MSI (`msi`) behalten das einfache Format. Nur die
//! DMA-Umleitung wird eingeschaltet.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// VT-d-Register (Spec §10.4), Offsets ab der Einheiten-Basis
const VER: u64 = 0x00;
const CAP: u64 = 0x08;
const ECAP: u64 = 0x10;
const GCMD: u64 = 0x18;
const GSTS: u64 = 0x1c;
const RTADDR: u64 = 0x20;
const CCMD: u64 = 0x28;
const FSTS: u64 = 0x34;

// GCMD/GSTS-Bits
const TE: u32 = 1 << 31; // Translation Enable
const SRTP: u32 = 1 << 30; // Set Root Table Pointer
const TES: u32 = 1 << 31; // Translation Enable Status
const RTPS: u32 = 1 << 30; // Root Table Pointer Status

static ON: AtomicBool = AtomicBool::new(false);
/// Phys. Adresse der gemeinsamen Wurzeltabelle (einmal gebaut, von allen Einheiten genutzt).
static ROOT: AtomicU64 = AtomicU64::new(0);
/// Registerbasis der ersten Einheit (für die Fehlerstatus-Abfrage).
static BASE0: AtomicU64 = AtomicU64::new(0);

fn rd32(base: u64, off: u64) -> u32 {
    // SICHERHEIT: `base` ist die von der DMAR-Tabelle genannte, identisch und ungepuffert abgebildete
    // Registerseite der VT-d-Einheit; ausgerichteter 32-Bit-Zugriff.
    unsafe { core::ptr::read_volatile((base + off) as *const u32) }
}
fn rd64(base: u64, off: u64) -> u64 {
    unsafe { core::ptr::read_volatile((base + off) as *const u64) }
}
fn wr32(base: u64, off: u64, v: u32) {
    unsafe { core::ptr::write_volatile((base + off) as *mut u32, v) }
}
fn wr64(base: u64, off: u64, v: u64) {
    unsafe { core::ptr::write_volatile((base + off) as *mut u64, v) }
}

fn zero_page() -> Option<u64> {
    let p = crate::mem::alloc_page_kernel()?;
    // SICHERHEIT: frisch zugeteilte, identisch abgebildete Seite; auf null setzen vor Gebrauch.
    unsafe { core::ptr::write_bytes(p as *mut u8, 0, 4096) };
    Some(p)
}

/// Baut die gemeinsame zweite Übersetzungsebene: `[0, ram_top)` mit 2-MiB-Seiten, Lesen+Schreiben.
/// `levels` = Tiefe der Tabelle (3 bei 39-Bit, 4 bei 48-Bit). Gibt die Wurzel (phys) zurück.
fn build_slpt(ram_top: u64, levels: u32) -> Option<u64> {
    let root = zero_page()?;
    // Index einer Adresse auf Ebene `lvl` (1 = unterste 4-KiB-Ebene). 2-MiB-Seiten sitzen auf Ebene 2.
    let idx = |gpa: u64, lvl: u32| ((gpa >> (12 + 9 * (lvl - 1))) & 0x1ff) as u64;
    let top = (ram_top + (2 << 20) - 1) & !((2 << 20) - 1);
    let mut gpa = 0u64;
    while gpa < top {
        // Von der Wurzel (Ebene = levels) bis Ebene 3 hinabsteigen, Tabellen bei Bedarf anlegen.
        let mut tbl = root;
        let mut lvl = levels;
        while lvl > 2 {
            let e = tbl + idx(gpa, lvl) * 8;
            let mut ent = unsafe { core::ptr::read_volatile(e as *const u64) };
            if ent & 1 == 0 {
                let next = zero_page()?;
                ent = next | 0b11; // Lesen+Schreiben, zeigt auf nächste Tabelle
                unsafe { core::ptr::write_volatile(e as *mut u64, ent) };
            }
            tbl = ent & 0x000f_ffff_ffff_f000;
            lvl -= 1;
        }
        // Ebene 2: 2-MiB-Großseite (PS = Bit 7), Lesen+Schreiben, 1:1
        let e = tbl + idx(gpa, 2) * 8;
        unsafe { core::ptr::write_volatile(e as *mut u64, (gpa & !((2 << 20) - 1)) | (1 << 7) | 0b11) };
        gpa += 2 << 20;
    }
    Some(root)
}

/// Baut Wurzel- und Kontexttabelle: **jede** Bus/Gerät/Funktion zeigt auf die gemeinsame zweite
/// Ebene (Standard-Domäne 1). `aw` = Adressbreiten-Feld der Kontexteinträge (001=39, 010=48 Bit).
fn build_root(slpt: u64, aw: u64) -> Option<u64> {
    let ctx = zero_page()?; // eine Kontexttabelle, von allen Bussen geteilt (256 Einträge)
    for d in 0..256u64 {
        // Kontexteintrag: present | TT=00 (nur zweite Ebene) | SLPTPTR; obere 64: DID=1 | AW
        unsafe {
            core::ptr::write_volatile((ctx + d * 16) as *mut u64, slpt | 1);
            core::ptr::write_volatile((ctx + d * 16 + 8) as *mut u64, (1u64 << 8) | aw);
        }
    }
    let root = zero_page()?;
    for b in 0..256u64 {
        // Wurzeleintrag je Bus: present | Zeiger auf die (geteilte) Kontexttabelle
        unsafe { core::ptr::write_volatile((root + b * 16) as *mut u64, ctx | 1) };
    }
    Some(root)
}

fn warte(base: u64, bit: u32) -> bool {
    for _ in 0..100_000 {
        if rd32(base, GSTS) & bit != 0 {
            return true;
        }
        crate::arch::pause();
    }
    false
}

/// Eine VT-d-Einheit einschalten: Wurzeltabelle setzen, Zwischenspeicher leeren, Übersetzung an.
fn enable_unit(base: u64) -> Result<(), &'static str> {
    let root = ROOT.load(Ordering::Relaxed);
    // Wurzeltabelle (Legacy-Format, nicht erweitert)
    wr64(base, RTADDR, root);
    wr32(base, GCMD, SRTP);
    if !warte(base, RTPS) {
        return Err("Wurzeltabelle nicht übernommen");
    }
    // Kontext-Zwischenspeicher global leeren (CCMD: ICC=Bit63, global=01b in Bit 61:60)
    wr64(base, CCMD, 1u64 << 63 | 1u64 << 60);
    for _ in 0..100_000 {
        if rd64(base, CCMD) & (1 << 63) == 0 {
            break;
        }
        crate::arch::pause();
    }
    // IOTLB global leeren (Register-Offset aus ECAP.IRO, Bit 63 IVT, global=01b in Bit 61:60)
    let iro = ((rd64(base, ECAP) >> 8) & 0x3ff) * 16;
    wr64(base, iro + 8, 1u64 << 63 | 1u64 << 60);
    for _ in 0..100_000 {
        if rd64(base, iro + 8) & (1 << 63) == 0 {
            break;
        }
        crate::arch::pause();
    }
    // Übersetzung einschalten
    wr32(base, GCMD, TE);
    if !warte(base, TES) {
        return Err("Übersetzung nicht eingeschaltet");
    }
    Ok(())
}

/// DMAR lesen, zweite Ebene und Wurzeltabelle bauen, alle Einheiten einschalten. Nach `pci::scan`,
/// vor `bind_all` (bevor Treiber mit DMA beginnen). `ram_top` = oberste Adresse des nutzbaren RAM.
pub fn init(rsdp: u64, ram_top: u64) {
    // SICHERHEIT: find_table liest nur die von der Firmware übergebenen ACPI-Tabellen.
    let Some(t) = (unsafe { crate::apic::find_table(rsdp, b"DMAR") }) else {
        crate::kprintln!("[iommu] kein DMAR – ohne DMA-Schutz (kein VT-d)");
        return;
    };
    let len = unsafe { core::ptr::read_unaligned((t + 4) as *const u32) } as u64;

    // Erste Einheit lesen, um Fähigkeiten (Adressbreite, Registerbasis) zu bestimmen.
    // DMAR-Kopf 48 Byte, dann Remapping-Strukturen: Typ u16, Länge u16; Typ 0 = DRHD.
    let mut off = 48u64;
    let mut first_cap = 0u64;
    let mut bases = [0u64; 8];
    let mut n = 0usize;
    while off + 4 <= len && n < bases.len() {
        let typ = unsafe { core::ptr::read_unaligned((t + off) as *const u16) };
        let slen = unsafe { core::ptr::read_unaligned((t + off + 2) as *const u16) } as u64;
        if slen == 0 {
            break;
        }
        if typ == 0 {
            // DRHD: Flags u8 (off+4), reserviert u8, Segment u16, Registerbasis u64 (off+8)
            let base = unsafe { core::ptr::read_unaligned((t + off + 8) as *const u64) };
            if base != 0 {
                crate::paging::map_identity(base & !0xfff, (base & !0xfff) + 0x1000, true);
                if first_cap == 0 {
                    first_cap = rd64(base, CAP);
                }
                bases[n] = base;
                n += 1;
            }
        }
        off += slen;
    }
    if n == 0 {
        crate::kprintln!("[iommu] DMAR ohne Einheit – kein VT-d");
        return;
    }
    // Adressbreite aus SAGAW (CAP Bit 9 = 39 Bit/3 Ebenen, Bit 10 = 48 Bit/4 Ebenen) wählen
    let sagaw = (first_cap >> 8) & 0x1f;
    let (levels, aw) = if sagaw & (1 << 1) != 0 {
        (3u32, 0b001u64)
    } else if sagaw & (1 << 2) != 0 {
        (4u32, 0b010u64)
    } else {
        crate::kprintln!("[iommu] keine unterstützte Adressbreite (SAGAW {sagaw:#x}) – kein VT-d");
        return;
    };
    let Some(slpt) = build_slpt(ram_top, levels) else {
        crate::kprintln!("[iommu] kein Speicher für Seitentabellen – kein VT-d");
        return;
    };
    let Some(root) = build_root(slpt, aw) else {
        crate::kprintln!("[iommu] kein Speicher für die Wurzeltabelle – kein VT-d");
        return;
    };
    ROOT.store(root, Ordering::Relaxed);
    let mut ok = 0;
    for &base in &bases[..n] {
        match enable_unit(base) {
            Ok(()) => ok += 1,
            Err(e) => crate::kprintln!("[iommu] Einheit {base:#x}: {e}"),
        }
    }
    ON.store(ok > 0, Ordering::Relaxed);
    BASE0.store(bases[0], Ordering::Relaxed);
    let ver = rd32(bases[0], VER);
    crate::kprintln!(
        "[iommu] VT-d {}.{}: {ok}/{n} Einheit(en) an, DMA-Schutz für RAM bis {} MiB ({}-Ebenen-Tabelle)",
        (ver >> 4) & 0xf,
        ver & 0xf,
        ram_top >> 20,
        levels
    );
}

/// Ob der DMA-Schutz aktiv ist (für Protokoll/Selbsttest).
pub fn active() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Fehlerstatus der ersten Einheit (FSTS; Bit 1 = Primary Pending Fault, Bit 0 = Fault Overflow).
/// 0 = kein gemeldeter DMA-Fehler. Für Protokoll/Selbsttest.
pub fn fault_status() -> u32 {
    let base = BASE0.load(Ordering::Relaxed);
    if base == 0 {
        return 0;
    }
    rd32(base, FSTS)
}
