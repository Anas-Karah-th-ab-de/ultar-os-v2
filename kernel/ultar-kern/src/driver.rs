//! Treiberschnittstelle des Ultar-Kerns. Jeder Treiber (handgeschrieben oder von der Treiber-Werkstatt erzeugt)
//! meldet sich mit `Driver` an und stellt seine Geräte über die Klassen-Schnittstellen (Block, Netz …) bereit.
//! Der Kern kennt keine Linux-Treiber: alles hier ist eigener Rust-Code.

use crate::pci::PciDev;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

pub type Result<T> = core::result::Result<T, String>;

pub trait Driver: Sync {
    /// kurzer Name, z. B. "virtio-blk"
    fn name(&self) -> &'static str;
    /// Herkunft: "hand" (geschrieben) oder "werkstatt" (erzeugt)
    fn origin(&self) -> &'static str {
        "hand"
    }
    fn matches(&self, d: &PciDev) -> bool;
    /// Gerät einrichten und in die passende Klasse eintragen
    fn probe(&self, d: &PciDev) -> Result<()>;
}

pub trait BlockDev: Send {
    fn name(&self) -> String;
    fn sectors(&self) -> u64;
    fn sector_size(&self) -> usize {
        512
    }
    /// `buf.len()` muss ein Vielfaches der Sektorgröße sein
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<()>;
    /// Schreiben (Länge Vielfaches der Sektorgröße); Standard: nur lesbar
    fn write(&mut self, _lba: u64, _buf: &[u8]) -> Result<()> {
        Err("Datenträger nur lesbar".into())
    }
}

pub trait NetDev: Send {
    fn name(&self) -> String;
    fn mac(&self) -> [u8; 6];
    fn send(&mut self, frame: &[u8]) -> Result<()>;
    fn recv(&mut self, buf: &mut [u8]) -> Option<usize>;
}

pub static BLOCK: Mutex<Vec<Box<dyn BlockDev>>> = Mutex::new(Vec::new());
pub static NET: Mutex<Vec<Box<dyn NetDev>>> = Mutex::new(Vec::new());

/// Ergebnis der Treibersuche je Gerät (für Anzeige und Werkstatt)
pub struct Bound {
    pub dev: PciDev,
    pub driver: Option<&'static str>,
    pub error: Option<String>,
}

pub static DEVICES: Mutex<Vec<Bound>> = Mutex::new(Vec::new());

pub fn bind_all(devs: Vec<PciDev>) {
    let mut out = Vec::new();
    for d in devs {
        let mut b = Bound { dev: d.clone(), driver: None, error: None };
        if let Some(drv) = crate::drivers::ALL.iter().find(|x| x.matches(&d)) {
            match drv.probe(&d) {
                Ok(()) => {
                    crate::kprintln!("[treiber] {} -> {} ({}) ok", d.id(), drv.name(), drv.origin());
                    b.driver = Some(drv.name());
                }
                Err(e) => {
                    crate::kprintln!("[treiber] {} -> {} FEHLER: {e}", d.id(), drv.name());
                    b.error = Some(e);
                }
            }
        }
        out.push(b);
    }
    *DEVICES.lock() = out;
}
