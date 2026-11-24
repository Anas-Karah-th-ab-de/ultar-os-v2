//! `ultar-calendard` – the calendar service of Ultar OS (area 4, system services).
//!
//! Keeps the events of each person in a checked state file (`<dir>/<name>.cal`,
//! atomic, CRC-32, at most 4096 events) and answers for the person in front
//! (`session.state`): `calendar.day` (the shell's calendar sheet), `calendar.add` /
//! `calendar.remove` (the calendar app). `calendar.changed` tells followers to ask again.
//! A damaged file is never overwritten: that person's calendar answers `Unavailable`
//! until it is repaired.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod book;
pub mod service;

pub use book::Book;
pub use service::CalendarService;
