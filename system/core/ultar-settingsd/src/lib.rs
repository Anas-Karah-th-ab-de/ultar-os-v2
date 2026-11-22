//! # ultar-settingsd – the settings service of Ultar OS
//!
//! **Area:** system core (area 3 of the seven areas, plan §3.0).
//!
//! Principle of the desktop specification: *surface = app, state and rules
//! = service*. The Settings app, the quick settings and the `usettings`
//! command only show what this service offers.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

pub mod access;
pub mod apply;
pub mod cli;
pub mod clock;
pub mod core;
pub mod daemon;
pub mod displays;
pub mod error;
pub mod i18n;
pub mod schema;
pub mod seed;
pub mod server;
pub mod store;
pub mod testdir;
pub mod value;
