//! `ultar-svckit` – the shared frame of the small system services (area 3, system core).
//!
//! The services behind the desktop shell's quick settings and power menu (power,
//! notifications, calendar, session, audio, media, removable drives, screenshots,
//! network, Bluetooth, VPN) all need the same frame; it lives here once:
//!
//! | Part | Module |
//! |---|---|
//! | Command line `--bus --log-socket --token-file --allow-root` + service options | [`args`] |
//! | Start (hardening, kernel rights, logger), bus loop with reconnect, requests, subscriptions, ticks | [`serve`] |
//! | State files: atomic write (temp + fsync + rename + directory fsync), CRC-32, size limit | [`store`] |
//! | Who is an administrator / system component (policy pseudo methods) | [`access`] |
//! | Texts of the command line (ar/en/de, Arabic first) | [`msg`] |
//!
//! A service implements [`serve::Service`]: it answers calls, receives the topics it
//! subscribed to, and is ticked for hardware it has to poll. Everything it says on the
//! bus is typed (`ultar_bus::messages`); everything it logs is a fixed key.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod access;
pub mod args;
pub mod msg;
pub mod serve;
pub mod store;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use access::Access;
pub use args::{CommonArgs, UsageError};
pub use serve::{main, main_pair, Bus, Poster, Service};
