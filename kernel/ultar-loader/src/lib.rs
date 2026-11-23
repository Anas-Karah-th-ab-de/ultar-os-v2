//! # ultar-loader – the A/B start stage of Ultar OS
//!
//! **Area:** kernel (start chain). The firmware starts this small UEFI program from
//! `ULTAR-EFI` (`\EFI\BOOT\BOOTX64.EFI`). It reads the slot table from `ULTAR-BOOTCTL`, takes
//! the start decision of `ultar_slots::table::select` (boot counter, automatic fallback),
//! stores that decision durably (write + flush) **before** anything else happens, then loads the
//! kernel image of the chosen slot from `ultar-kernel-<slot>` and starts it with the load
//! options `ultar.slot=<a|b>` (+ `ultar.trial=1` for a counted attempt).
//!
//! This library is the whole decision logic over a [`disk::Disk`] and is tested on the host;
//! `main.rs` only binds it to UEFI (Block I/O, `LoadImage`, `StartImage`). The slot table,
//! the decision and the write placement are the same code the updater uses (`ultar-slots`).
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

pub mod boot;
pub mod disk;
pub mod msg;
pub mod pe;
