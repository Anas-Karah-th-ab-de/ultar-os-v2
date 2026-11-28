//! MSI und MSI-X: nachrichtensignalisierte Interrupts. Statt eine geteilte INTx-Leitung zu ziehen,
//! schreibt ein Gerät einen gewählten Wert (den Vektor) an eine gewählte Adresse (ein lokaler APIC
//! eines gewählten Kerns). So bekommt jedes Gerät – und jede Warteschlange eines Geräts (NVMe, die
//! 10G-Karte) – einen eigenen Vektor samt Handler, und der Interrupt landet auf dem Kern, der auf ihn
//! wartet. Der Grafiktreiber richtete sich seinen MSI früher selbst ein (`drivers::intel_display::
//! gt_irq`); dieses Modul ist der gemeinsame Weg für alle übrigen Geräte.
//!
//! Unterbrechungs-Umleitung (interrupt remapping) der IOMMU bleibt aus: die MSI behalten das
//! einfache Format (Ziel-APIC in der Adresse, Vektor in den Daten, Intel SDM Bd. 3 §11.11). Die
//! DMA-Umleitung der IOMMU (`iommu`) berührt das nicht.

use crate::pci::PciDev;
use spin::Mutex;

/// Erster und letzter frei vergebbare Vektor. Darunter liegen die festen Vektoren (Timer 32,
/// Grafik 0x41, Aufwecken 0x42, Syscall 0x80); 0xff ist „spurious“. Der Bereich 0x90–0xEF meidet den
/// Syscall-Vektor 0x80 und den spurious-Vektor und fasst 96 Vektoren.
const POOL_FIRST: u8 = 0x90;
const POOL_LAST: u8 = 0xef;
const POOL_LEN: usize = (POOL_LAST - POOL_FIRST + 1) as usize;

/// Lokale-APIC-Nachrichtenadresse (Intel SDM Bd. 3 §11.11.1): physisches Ziel in Bit 19:12.
const MSI_ADDRESS_BASE: u32 = 0xfee0_0000;

#[allow(dead_code)] // Einzel-MSI-Pfad (program_msi), bis NVMe/xHCI ihn nutzen – siehe docs/OFFEN-WARNUNGEN.md
const PCI_CAP_MSI: u8 = 0x05;
const PCI_CAP_MSIX: u8 = 0x11;

/// Je Pool-Vektor: der registrierte Handler. Er bekommt seinen eigenen Vektor übergeben, damit ein
/// Treiber mit mehreren Warteschlangen (NVMe, 10G) erkennt, welche Warteschlange ihn geweckt hat.
/// Wie bei `gt_irq` wird die Sperre auch im Interrupt kurz genommen (Vergabe nur beim Einrichten,
/// Zustellung nur lesend).
static HANDLERS: Mutex<[Option<fn(u8)>; POOL_LEN]> = Mutex::new([None; POOL_LEN]);

/// Nächster zu vergebender Index (einfacher fortlaufender Zeiger, es werden nie welche frei, außer
/// ein Gerät gibt sie beim Abbau zurück – dann bleibt die Lücke bis zum nächsten Rundlauf).
fn find_free(table: &[Option<fn(u8)>; POOL_LEN]) -> Option<usize> {
    table.iter().position(|h| h.is_none())
}

/// Einen Vektor vergeben und seinen Handler eintragen. Gibt den x86-Vektor (Pool-Bereich) zurück
/// oder `None`, wenn der Pool voll ist. Der Handler läuft im Interrupt; danach ruft die Zustellung
/// selbst `apic::eoi()`.
pub fn alloc(handler: fn(u8)) -> Option<u8> {
    let mut t = HANDLERS.lock();
    let i = find_free(&t)?;
    t[i] = Some(handler);
    Some(POOL_FIRST + i as u8)
}

/// Einen Vektor zurückgeben (Gerät abgebaut). Ein danach eintreffender später Interrupt findet
/// keinen Handler und wird nur quittiert.
pub fn free(vector: u8) {
    if (POOL_FIRST..=POOL_LAST).contains(&vector) {
        HANDLERS.lock()[(vector - POOL_FIRST) as usize] = None;
    }
}

/// Ob ein Vektor im vergebbaren Bereich liegt (für die Fallunterscheidung im Trap).
pub fn in_pool(vector: u8) -> bool {
    (POOL_FIRST..=POOL_LAST).contains(&vector)
}

/// Zustellung aus dem Trap: den Handler des Vektors rufen (falls vorhanden). Quittierung macht der
/// Aufrufer.
pub fn dispatch(vector: u8) {
    if !in_pool(vector) {
        return;
    }
    let h = HANDLERS.lock()[(vector - POOL_FIRST) as usize];
    if let Some(f) = h {
        f(vector);
    }
}

/// Die MSI-Nachrichtenadresse für einen Zielkern (dessen APIC-Kennung).
fn msg_address(dest_apic: u32) -> u32 {
    MSI_ADDRESS_BASE | (dest_apic & 0xff) << 12
}

/// Position der MSI- bzw. MSI-X-Fähigkeit im Konfigurationsraum (0 = keine).
fn cap_offset(dev: &PciDev, want: u8) -> Option<u8> {
    for (pos, id) in dev.caps() {
        if id == want {
            return Some(pos);
        }
    }
    None
}

/// Richtet den **MSI** eines Geräts auf einen Kern und einen Vektor ein (ein einzelner Vektor).
/// Rückgabe: Ok, wenn das Gerät MSI hat und es eingeschaltet wurde.
#[allow(dead_code)] // Einzel-MSI-Pfad für Geräte ohne MSI-X (NVMe/xHCI folgen) – siehe docs/OFFEN-WARNUNGEN.md
pub fn program_msi(dev: &PciDev, vector: u8, dest_apic: u32) -> Result<(), &'static str> {
    let cap = cap_offset(dev, PCI_CAP_MSI).ok_or("keine MSI-Fähigkeit")?;
    let head = dev.r32(cap);
    let control = (head >> 16) as u16;
    let wide = control & 1 << 7 != 0;
    dev.w32(cap + 4, msg_address(dest_apic));
    let data_at = if wide {
        dev.w32(cap + 8, 0); // obere 32 Bit der Adresse = 0
        cap + 12
    } else {
        cap + 8
    };
    // Flanke, feste Zustellung, nur der Vektor
    dev.w32(data_at, vector as u32);
    if control & 1 << 8 != 0 {
        // vektorweise Maske vorhanden: Vektor 0 durchlassen
        dev.w32(data_at + 4, 0);
    }
    // ein Vektor (multiple message enable = 0), MSI an (Bit 0)
    let control = (control & !(7 << 4)) | 1;
    dev.w32(cap, (head & 0xffff) | (control as u32) << 16);
    Ok(())
}

/// Ein Eintrag der MSI-X-Tabelle: welche Tabellenzeile auf welchen Vektor zeigt.
pub struct MsixEntry {
    pub index: u16,
    pub vector: u8,
}

/// Richtet **MSI-X** ein: schreibt für jede genannte Tabellenzeile Adresse (Zielkern), Daten
/// (Vektor) und hebt die Maske auf; danach wird MSI-X am Gerät eingeschaltet und die Funktionsmaske
/// gelöscht. Nötig für mehrere Warteschlangen (NVMe, 10G). Die Tabelle liegt in einem BAR des
/// Geräts; dieser wird identisch und ungepuffert abgebildet (wie die übrigen Geräteregister).
pub fn program_msix(dev: &PciDev, entries: &[MsixEntry], dest_apic: u32) -> Result<(), &'static str> {
    let cap = cap_offset(dev, PCI_CAP_MSIX).ok_or("keine MSI-X-Fähigkeit")?;
    let control = (dev.r32(cap) >> 16) as u16;
    let table_size = (control & 0x7ff) + 1;
    // Tabellen-Zeiger: BAR-Index in Bit 2:0, Offset (8-ausgerichtet) im Rest.
    let tbl = dev.r32(cap + 4);
    let bir = (tbl & 0x7) as usize;
    let tbl_off = (tbl & !0x7) as u64;
    let bar = dev.bars.get(bir).and_then(|b| b.as_ref()).ok_or("MSI-X-BAR fehlt")?;
    if bar.io {
        return Err("MSI-X-BAR ist ein E/A-Fenster");
    }
    let tbl_base = bar.addr + tbl_off;
    if tbl_off + (table_size as u64) * 16 > bar.size {
        return Err("MSI-X-Tabelle außerhalb des BAR");
    }
    // Den Tabellenbereich erreichbar machen (ungepuffert über die MTRRs des MMIO-Lochs).
    crate::paging::map_identity(tbl_base & !0xfff, (tbl_base + bar.size - tbl_off + 0xfff) & !0xfff, true);
    for e in entries {
        if e.index >= table_size {
            return Err("MSI-X-Zeile außerhalb der Tabelle");
        }
        let row = tbl_base + e.index as u64 * 16;
        // SICHERHEIT: `row` liegt innerhalb des oben geprüften und abgebildeten MSI-X-Tabellen-
        // bereichs des BAR; 32-Bit-volatile-Schreibzugriffe sind der vom Gerät erwartete Zugang.
        unsafe {
            core::ptr::write_volatile(row as *mut u32, msg_address(dest_apic)); // Adresse unten
            core::ptr::write_volatile((row + 4) as *mut u32, 0); // Adresse oben
            core::ptr::write_volatile((row + 8) as *mut u32, e.vector as u32); // Daten = Vektor
            core::ptr::write_volatile((row + 12) as *mut u32, 0); // Maske aus
        }
    }
    // MSI-X an (Bit 15), Funktionsmaske aus (Bit 14).
    let new = (control & !(1 << 14)) | (1 << 15);
    let head = dev.r32(cap);
    dev.w32(cap, (head & 0xffff) | (new as u32) << 16);
    Ok(())
}

/// Ob ein Gerät MSI-X anbietet.
pub fn has_msix(dev: &PciDev) -> bool {
    cap_offset(dev, PCI_CAP_MSIX).is_some()
}

/// Ob ein Gerät MSI anbietet.
#[allow(dead_code)] // zusammen mit program_msi (NVMe/xHCI folgen) – siehe docs/OFFEN-WARNUNGEN.md
pub fn has_msi(dev: &PciDev) -> bool {
    cap_offset(dev, PCI_CAP_MSI).is_some()
}
