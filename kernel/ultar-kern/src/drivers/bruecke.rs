//! Brücken (PCI-Klasse 06): Host-Brücke, ISA/LPC-Brücke, PCI-PCI-Brücken. Sie brauchen keine eigene Logik –
//! die Firmware hat Fenster und Busnummern vergeben, der Kern zählt die Busse dahinter selbst auf. Der Treiber
//! übernimmt sie, damit jedes Gerät einen Besitzer hat, und schaltet bei PCI-PCI-Brücken die Weiterleitung ein.

use crate::driver::{Driver, Result};
use crate::pci::PciDev;

pub struct Drv;
pub static DRV: Drv = Drv;

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "bruecke"
    }
    fn matches(&self, d: &PciDev) -> bool {
        d.class == 0x06
    }
    fn probe(&self, d: &PciDev) -> Result<()> {
        if d.subclass == 0x04 {
            // E/A, Speicher und Bus-Master durchreichen (Geräte hinter der Brücke)
            let cmd = d.r32(0x04) & 0xffff;
            d.w32(0x04, cmd | 0x0007);
        }
        Ok(())
    }
}
