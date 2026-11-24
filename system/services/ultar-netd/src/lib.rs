//! `ultar-netd` – the network service of Ultar OS (area 4, system services).
//!
//! Publishes `net.state` (retained): the wired link and its speed, the Wi-Fi interface
//! and its radio, airplane mode. Switches the Wi-Fi radio and airplane mode through the
//! kernel's radio switches (HAL [`ultar_hal_linux::rfkill`]); a hardware switch is
//! reported, never overridden. The wanted state is kept in a checked file and restored
//! at start. Listing and joining Wi-Fi networks needs a Wi-Fi supplicant, which this
//! release does not have: `net.scan` / `net.connect` answer `Unsupported` and the state
//! says `networks_supported: false`.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod service;

pub use service::NetService;
