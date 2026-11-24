//! Alle Treiber des Ultar-Kerns. Die Treiber-Werkstatt trägt erzeugte Treiber zwischen den Markierungen ein.

pub mod bruecke;
pub mod intel_display;
pub mod nvidia;
pub mod nvme;
pub mod ps2kbd;
pub mod ps2maus;
pub mod nvidia_display;
pub mod virtio;
pub mod virtio_blk;
pub mod virtio_net;
pub mod virtio_rng;
// WERKSTATT-MODULE-BEGINN
pub mod gen_ahci;
pub mod gen_bochs_vga;
pub mod gen_hda;
pub mod gen_intel_eth;
pub mod gen_ixgbe;
pub mod gen_smbus;
pub mod gen_xhci;
// WERKSTATT-MODULE-ENDE

use crate::driver::Driver;

pub static ALL: &[&dyn Driver] = &[
    &virtio_blk::DRV,
    &virtio_net::DRV,
    &nvme::DRV,
    &virtio_rng::DRV,
    &bruecke::DRV,
    &intel_display::DRV,
    &nvidia_display::DRV,
    // WERKSTATT-TREIBER-BEGINN
    &gen_ahci::DRV,
    &gen_bochs_vga::DRV,
    &gen_hda::DRV,
    &gen_intel_eth::DRV,
    &gen_ixgbe::DRV,
    &gen_smbus::DRV,
    &gen_xhci::DRV,
    // WERKSTATT-TREIBER-ENDE
];
