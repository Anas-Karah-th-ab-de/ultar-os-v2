//! # ultar-intel-display – the Intel display engine, as pure logic
//!
//! **Area:** drivers (area 2, plan §3.0 and §5), kernel part. The Ultar kernel's Intel display
//! driver (`kernel/ultar-kern/src/drivers/intel_display/`) is split in two:
//!
//! * this crate: everything that can be written without touching hardware – register layout,
//!   decoding the state the firmware (UEFI GOP) left behind, display timings, GGTT entries,
//!   vertical-blank counting, pipe CRC handling, hot-plug decoding, DisplayPort AUX messages,
//!   DPCD, EDID, the link-training state machine, link bandwidth (M/N), the C20 PHY PLL tables and
//!   the mode-set sequences. Sequences that need register access are generic over [`hw::Hw`], so
//!   they run in the kernel against MMIO and in the tests against a simulated register file;
//! * the kernel module: MMIO mapping, memory for scan-out buffers, the PCI driver and the self test.
//!
//! Hardware: display version 14 (Meteor Lake / Arrow Lake-H, "Xe_LPD+", e.g. Arc 140T 8086:7d51 of
//! the Beelink GTi15 Ultra) fully; display version 9 (Comet Lake UHD 630 of the ThinkPad P1 Gen 3)
//! for taking over the firmware state (same plane, pipe and interrupt blocks), no mode set.
//!
//! Sources: Intel's public graphics PRMs and the MIT-licensed Linux i915 display code (register
//! offsets, the C20 PLL tables and the order of the PHY and link-training steps were taken from
//! it; files that follow it closely say so in their header). No GPL code.
//!
//! Everything is `no_std` with `alloc`, without `unsafe`, and tested on the host.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

extern crate alloc;

pub mod cdclk;
pub mod clocks;
pub mod crc;
pub mod dp;
pub mod edid;
pub mod error;
pub mod ggtt;
pub mod gmbus;
pub mod hotplug;
pub mod hw;
pub mod init;
pub mod mode;
pub mod modeset;
pub mod optin;
pub mod phy;
pub mod plane;
pub mod platform;
pub mod power;
pub mod readout;
pub mod regs;
pub mod vblank;
pub mod vbt;
pub mod wm;

#[cfg(test)]
pub mod sim;

#[cfg(test)]
mod review_tests;

pub use error::Error;
pub use hw::Hw;
pub use mode::Timing;
pub use platform::Platform;
