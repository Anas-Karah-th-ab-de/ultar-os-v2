//! `ultar-btd` – the Bluetooth service of Ultar OS (area 4, system services).
//!
//! Publishes `bluetooth.state` (retained): whether the device has Bluetooth (a radio
//! switch of that type or a controller the kernel lists) and whether its radio is on;
//! `bluetooth.set` switches the radio through the kernel's radio switch (a hardware
//! switch is reported, never overridden); the wish is kept and restored at start.
//! Pairing and connecting devices needs a Bluetooth host stack, which this release does
//! not have: `devices` / `connect` / `disconnect` answer `Unsupported` and the state
//! says `devices_supported: false`.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod service;

pub use service::BtService;
