//! # ultar-launcher – every app in its own cell
//!
//! **Area:** system core (area 3, PLAN.md §3.0); the step "apps as own
//! processes in a cell with rights from the manifest" of phase P5 (§8),
//! built on the kernel's isolation (§3.7: namespaces, groups, rights).
//!
//! What every app gets:
//!
//! * its **own user id**, from a reserved range, kept for ever (also after
//!   uninstalling; never given to another app) – [`store`];
//! * its **own data directory** (`0700`, owned by its user) – [`datadir`];
//! * its **own memory group** `apps/<app>` with the limits of its manifest,
//!   capped by its profile – [`profile`], [`plan`];
//! * a **cell**: mount + pid namespaces, a read-only view of the system,
//!   a private `/tmp`, its data directory as the only writable place,
//!   no network unless granted – [`cell`];
//! * **no kernel rights** and exactly the permissions its signed manifest
//!   declares and the user granted – [`manifest`], [`perm`]; its `ui.*`
//!   permissions reach the compositor through [`rightsfile`].
//!
//! Requests come over the bus (`ultar_bus::messages::launcher`, served by
//! [`service`]); the shell is the caller, `ulaunch` the administrator's tool.
//! Starts are single-instance per app and role, activation tokens of the
//! compositor are handed to the app, background services are restarted
//! after their policy ([`policy`]), output goes to the log service under
//! the app's name ([`appid::AppId::log_name`]).
//!
//! Works on Linux (development) and on the Ultar kernel: what the kernel
//! lacks is detected ([`platform`]) and stops a start unless the operator
//! accepted it (fail closed).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

pub mod appid;
pub mod areas;
pub mod atomic;
pub mod cell;
pub mod cleanup;
pub mod config;
pub mod datadir;
pub mod defaults;
pub mod error;
pub mod follow;
pub mod install;
mod instances;
pub mod launcher;
pub mod manifest;
pub mod msg;
pub mod open;
pub mod perm;
pub mod plan;
pub mod platform;
pub mod policy;
pub mod profile;
pub mod rightsfile;
pub mod service;
pub mod spawn;
pub mod store;
mod timer;
pub mod version;

pub use error::{LaunchError, PackageError};
pub use launcher::Launcher;
