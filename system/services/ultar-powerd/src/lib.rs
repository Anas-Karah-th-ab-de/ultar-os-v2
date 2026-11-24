//! `ultar-powerd` – the power service of Ultar OS (area 4, system services).
//!
//! * Publishes `power.state` (battery, charger, temperature; retained) and
//!   `power.actions` (what the power menu can offer on this device).
//! * `power.suspend`: locks the screen first when someone is signed in (fail closed:
//!   no lock, no sleep), then suspends to memory – only when the kernel offers it,
//!   otherwise `Unsupported` (in QEMU and on the Ultar kernel today).
//! * `power.restart` / `power.off`: through the service manager (`init.power`), which
//!   ends every service in order. While other people are signed in too, an
//!   administrator's password is asked for in the system dialog first.
//!
//! Who may call: system components with the pseudo method `power.system` (shell, lock
//! screen) and administrators (`power.admin`); apps never (the broker refuses them,
//! and so does this service).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod service;
pub mod state;

pub use service::PowerService;
