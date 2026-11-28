//! # ultar-foreign – apps from other systems (`docs/FREMD-APPS.md`)
//!
//! **Area:** system services (area 4). Takes a file of the person (`.deb`; `.exe`,
//! `.msi` and `.apk` follow in later stages) or a package name, asks an administrator
//! in the system dialog, installs it into the shared **Linux area** in a cell
//! ([`cellrun`]) and registers each of its visible programs with the launcher as an app
//! of the profile `app.foreign` ([`jobs`], [`harvest`], [`state`]).
//!
//! What it never does: parse a package's insides itself (the Linux area's own tools
//! do, in a cell without network or rights), run anything of a package outside a cell,
//! install without the administrator's confirmation, or give an app more than the
//! person chose (network: none or internet only; folders one by one).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

pub mod cellrun;
pub mod desktop;
pub mod detect;
pub mod harvest;
pub mod jobs;
pub mod layout;
pub mod msi;
pub mod pe;
pub mod service;
pub mod state;
pub mod windows;
pub mod winread;

pub use service::ForeignService;
