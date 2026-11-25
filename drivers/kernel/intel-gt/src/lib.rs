//! # ultar-intel-gt – the render/compute side of the Intel integrated GPU, as pure logic
//!
//! **Area:** drivers (area 2), kernel part. `ultar-intel-display` drives the display engine of
//! the device; this crate drives the *GT* of the same PCI device (same BAR0): the engines that
//! run 3D and compute work. It is the base of hardware acceleration for the compositor and of
//! local models on the integrated GPU (plan: `docs/integration/intel-gt.md`).
//!
//! Stages G0 to G3 (first job on an engine) of that plan live here:
//!
//! * [`ident`] – which graphics and media IP the device has (GMD_ID),
//! * [`forcewake`] – waking the GT power domains before their registers are touched,
//! * [`engines`] – which engines exist (descriptor of the IP and fuses),
//! * [`fw`] – the container of the GuC firmware (CSS header, versions, RSA key position),
//! * [`wopcm`] – the layout of the write-once protected memory the firmware runs in,
//! * [`guc`] – reset, boot parameters, RSA and DMA transfer, load status,
//! * [`ads`], [`params`], [`hxg`], [`hwconfig`] – the data block the firmware starts with, its boot
//!   parameters, messages over registers and the hardware configuration table,
//! * [`ctb`] – the command buffers (requests, responses, events),
//! * [`mi`], [`lrc`], [`submit`] – commands, the context image and the messages that make the
//!   GuC run a context on an engine,
//! * [`irq`] – interrupts of the engines and the GuC (a job ended, a message from the GuC),
//! * [`workarounds`] – hardware workarounds of Xe_LPG, programmed before programs run,
//! * [`slpc`] – the GuC's power management (frequency from the load, sleep when idle),
//! * [`topology`] – slices, execution units and cache banks of this chip (fuses),
//! * [`uapi`] – the interface for programs (Linux `xe` uAPI): request numbers and device queries,
//! * [`state`] – a read-out of all of that for the kernel log and the self test.
//!
//! Hardware: graphics 12.70/12.71/12.74 (Meteor Lake / Arrow Lake, "Xe_LPG", e.g. the Arc 140T
//! 8086:7d51 of the Beelink GTi15 Ultra) with media 13.00.
//!
//! Sources: the MIT-licensed Linux `xe` driver (register offsets, the firmware container and the
//! order of the upload steps were taken from it; files that follow it closely say so in their
//! header). No GPL code. The GuC firmware itself is Intel's (redistributable) and is only loaded.
//!
//! Everything is `no_std` with `alloc`, without `unsafe`, and tested on the host against a
//! simulated register file.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

extern crate alloc;

pub mod ads;
pub mod ctb;
pub mod engines;
pub mod error;
pub mod forcewake;
pub mod freq;
pub mod fw;
pub mod guc;
pub mod hwconfig;
pub mod hxg;
pub mod irq;
pub mod ident;
pub mod lrc;
pub mod mi;
pub mod params;
pub mod ppgtt;
pub mod regs;
pub mod state;
pub mod slpc;
pub mod submit;
pub mod topology;
pub mod uapi;
pub mod workarounds;
pub mod wopcm;

#[cfg(test)]
pub mod sim;

pub use error::{Error, Result};
pub use ultar_intel_display::hw::{field, genmask, prep, Hw};
