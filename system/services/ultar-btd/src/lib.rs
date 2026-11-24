//! `ultar-btd` – the Bluetooth service of Ultar OS (area 4, system services).
//!
//! Publishes `bluetooth.state` (retained): whether the device has Bluetooth (a radio
//! switch of that type or a usable controller) and whether its radio is on;
//! `bluetooth.set` switches the radio through the kernel's radio switch (a hardware
//! switch is reported, never overridden); the wish is kept and restored at start.
//!
//! With a controller (Linux: the HCI user channel) its own host stack ([`stack`], run
//! by [`runner`]) looks for devices, pairs (LE Secure Connections, classic Secure Simple
//! Pairing – each confirmed by the person in the system dialog), keeps bonds (0600),
//! connects bonded devices and hands keyboards and mice to the input subsystem.
//! Without one, `devices_supported` is `false` and the device calls answer `NoDevice`.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod runner;
pub mod service;
pub mod stack;

pub use service::BtService;
