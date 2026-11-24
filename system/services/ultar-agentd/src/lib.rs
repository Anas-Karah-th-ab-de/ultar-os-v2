//! `ultar-agentd` – the local helper of Ultar OS (area 4, system services).
//!
//! The person asks in words; the helper answers and does what was asked through the system
//! bus. It has no power of its own: every action is a bus call its policy rule allows, and
//! anything dangerous is confirmed by the person in the system dialog first.
//!
//! Where it thinks follows the device ([`brain`]): with the device's own model, with the
//! model of a paired device that lends it (the mesh), or – when there is neither – with
//! the smallest help ([`minimal`]): fixed commands understood without any model.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod brain;
pub mod help;
pub mod minimal;
pub mod model;
mod msg;
pub mod service;
pub mod session;
pub mod toolbox;
mod toolbox_more;
pub mod tools;

pub use service::AgentService;

/// The helper's texts (`agent.*`) by language tag, for the surface that shows its answers.
pub const LOCALES: [(&str, &str); 3] = [
    ("ar", include_str!("../locales/ar.ftl")),
    ("en", include_str!("../locales/en.ftl")),
    ("de", include_str!("../locales/de.ftl")),
];
