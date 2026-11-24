//! # ultar-slots – the A/B boot contract of Ultar OS
//!
//! **Area:** system core (area 3, plan §3.0). This crate is the one source of the data that the
//! system updater (`ultar-update`), the UEFI loader (`kernel/ultar-loader`), the installer and the
//! kernel (`kernel/ultar-kern`) exchange about the two system slots:
//!
//! * [`table`] – the 64-byte slot table (two copies) and [`table::select`], the start decision
//!   (format: `docs/SYSTEM-UPDATE.md` §2.5);
//! * [`layout`] – partition names, where the table copies and slot records lie on the
//!   `ULTAR-BOOTCTL` partition, the size rules, and the `ultar.slot=` load option;
//! * [`meta`] – the per-slot record (verity root hash) written by the updater;
//! * [`install`] – the installer's target layout, boot control image and partition table;
//! * [`gpt`] – a strict, allocation-free reader of GUID partition tables.
//!
//! Everything is pure (no I/O, no allocation, `no_std`), so the same code runs in the firmware
//! phase, in the kernel and in user space, and is tested once on the host.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

pub mod crc;
pub mod gpt;
pub mod install;
pub mod layout;
pub mod meta;
pub mod slot;
pub mod table;

pub use crc::crc32;
pub use slot::Slot;
pub use table::{plan_store, select, seq_newer, SlotFlags, SlotTable, TableError, TABLE_LEN};
