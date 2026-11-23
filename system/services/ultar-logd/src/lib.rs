//! # ultar-logd – the log service of Ultar OS
//!
//! **Area:** system services (area 4 of the seven areas, plan §3.0).
//! Closes the "no log service" part of review block B.
//!
//! * Services write typed records through `ultar_service::log` to the
//!   ingestion socket. The service name is **not** in the record: it is
//!   the identity the kernel credentials of the connection map to, so one
//!   service cannot write under another's name.
//! * The newest records live in a bounded ring in memory ([`ring`]); all
//!   records go to segment files with a size cap, rotation and retention
//!   by total size and age ([`store`]).
//! * Every stored record is a link in a hash chain ([`chain`]);
//!   [`verify`] and `ulog verify` find a changed, removed or inserted
//!   record.
//! * A full or broken disk never blocks a client and never stops the
//!   service; a flood from one source is cut by a rate limit per source
//!   with a "N records dropped" marker ([`core`]).
//! * Reading goes through the bus service `log` ([`service`]): an
//!   identity sees its own records; the permission `log.read_all` shows
//!   everything. The kernel log is a separate source, marked as such
//!   ([`kernel`]).
//!
//! The daemon is `ultar-logd`, the tool `ulog`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

pub mod chain;
pub mod cli;
pub mod clock;
pub mod core;
pub mod daemon;
pub mod export;
pub mod format;
pub mod ingest;
pub mod kernel;
pub mod msg;
pub mod record;
pub mod ring;
pub mod segment;
pub mod service;
pub mod storage;
pub mod store;
#[cfg(test)]
mod testkit;
pub mod verify;
