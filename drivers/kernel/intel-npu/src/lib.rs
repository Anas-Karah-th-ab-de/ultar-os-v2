//! # ultar-intel-npu – the Intel NPU ("AI Boost"), as pure logic
//!
//! **Area:** drivers (area 2), kernel part. The neural processing unit of Meteor Lake and Arrow
//! Lake (NPU 3720, PCI 8086:7d1d, e.g. in the Core Ultra 9 285H of the Beelink GTi15 Ultra) runs
//! inference work beside the processor and the integrated GPU. It is a small computer of its own
//! (a LEON processor with the inference accelerators) behind two register blocks:
//!
//! * the *buttress* (BAR4): power, clock (PLL work point), reset, low-power state D0i3,
//! * the *IP* (BAR0): the NPU's own subsystems; readable only while the NPU is powered.
//!
//! Stage N0 of the plan (`docs/integration/intel-npu.md`) lives here:
//!
//! * [`ident`] – device ids, generation, the address ranges the firmware expects,
//! * [`pll`] – frequency fuses and the work point request,
//! * [`power`] – the power-up sequence to a running IP, and power-down,
//! * [`fw`] – the container of the NPU firmware (header checks, versions, entry point),
//! * [`state`] – a read-only look at the buttress.
//!
//! Stage N1 (not here yet): the NPU's MMU and page tables, loading the firmware image, boot
//! parameters, the IPC channel and the job interface.
//!
//! Sources: there is no public register specification. The Linux `ivpu` driver (GPL) was read as
//! the *description* of the hardware – register offsets, bit meanings, the order of the steps;
//! the code here is written for the Ultar kernel from that description and copies none of it.
//! The NPU firmware is Intel's (redistributable) and is only loaded.
//!
//! Everything is `no_std` with `alloc`, without `unsafe`, and tested on the host against a
//! simulated register file.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

extern crate alloc;

pub mod error;
pub mod fw;
pub mod hw;
pub mod ident;
pub mod optin;
pub mod pll;
pub mod power;
pub mod regs;
pub mod state;

#[cfg(test)]
pub mod sim;

pub use error::{Error, Result};
pub use hw::{Hw, Reg, Space};
