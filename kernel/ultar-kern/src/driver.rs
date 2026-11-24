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
    /// Whether this driver has a teardown path (`remove`); `Err` names why not. A driver without
    /// one is never unbound: the device stays as it is (see `devices/`).
    fn can_remove(&self) -> core::result::Result<(), &'static str> {
        Err("no teardown path in this driver")
    }
    /// Quiesce the device after its class entries were detached: stop DMA and every activity of
    /// the hardware (reset it). The device layer then turns off bus mastering and decoding.
    fn remove(&self, _d: &PciDev) -> Result<()> {
        Err(String::from("no teardown path in this driver"))
    }
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
    /// Makes every completed write durable (the device's write cache is emptied). A driver
    /// that cannot guarantee that keeps this default, and `fsync` on its raw nodes fails.
    fn flush(&mut self) -> Result<()> {
        Err("no cache flush".into())
    }
}

pub trait NetDev: Send {
    fn name(&self) -> String;
    fn mac(&self) -> [u8; 6];
    fn send(&mut self, frame: &[u8]) -> Result<()>;
    fn recv(&mut self, buf: &mut [u8]) -> Option<usize>;
    /// Link state as the hardware reports it; `None`: the driver cannot tell.
    fn link(&self) -> Option<bool> {
        None
    }
    /// Link speed in Mbit/s while the link is up; `None`: the driver cannot tell.
    fn speed_mbit(&self) -> Option<u32> {
        None
    }
    /// Name of the driver behind the interface (`/sys/class/net/<if>/ultar_driver`).
    fn driver(&self) -> &'static str {
        "-"
    }
    /// A placeholder of an unbound device: not listed as an interface.
    fn detached(&self) -> bool {
        false
    }
}

pub static BLOCK: Mutex<Vec<Box<dyn BlockDev>>> = Mutex::new(Vec::new());
pub static NET: Mutex<Vec<Box<dyn NetDev>>> = Mutex::new(Vec::new());

/// Ergebnis der Treibersuche je Gerät (für Anzeige und Werkstatt)
pub struct Bound {
    pub dev: PciDev,
    pub driver: Option<&'static str>,
    pub error: Option<String>,
    /// The driver whose probe gave `error`.
    pub tried: Option<&'static str>,
    /// Turned off through `/sys/bus/pci/devices/<d>/enable`: no driver may bind it.
    pub disabled: bool,
    /// Class entries the bound driver created (indices into `NET` / `BLOCK`), see `devices/`.
    pub slots: crate::devices::Slots,
}

pub static DEVICES: Mutex<Vec<Bound>> = Mutex::new(Vec::new());

pub fn bind_all(devs: Vec<PciDev>) {
    let mut out = Vec::new();
    for d in devs {
        let mut b = Bound { dev: d.clone(), driver: None, error: None, tried: None, disabled: false, slots: crate::devices::Slots::default() };
        if let Some(drv) = crate::drivers::ALL.iter().find(|x| x.matches(&d)) {
            let (r, slots) = crate::devices::probe_with_slots(*drv, &d);
            b.slots = slots;
            match r {
                Ok(()) => {
                    crate::kprintln!("[treiber] {} -> {} ({}) ok", d.id(), drv.name(), drv.origin());
                    b.driver = Some(drv.name());
                }
                Err(e) => {
                    crate::kprintln!("[treiber] {} -> {} FEHLER: {e}", d.id(), drv.name());
                    b.error = Some(e);
                    b.tried = Some(drv.name());
                }
            }
        }
        out.push(b);
    }
    *DEVICES.lock() = out;
}
