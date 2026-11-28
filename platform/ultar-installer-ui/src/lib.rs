//! `ultar-installer-ui` – the graphical installer of Ultar OS (area 5 "platform", plan §3.0;
//! DESKTOP-SPEC §6, §13; boards `Installer`, `InstallerDisk`, `InstallerDrivers`,
//! `InstallerSummary`).
//!
//! Layers: [`wizard`] (the 16 steps, pure), [`components`] (step 8 rules), [`engine`] (disks:
//! inspect read-only, plan with `ultar_slots::install`, write only with an
//! [`engine::EraseConfirmation`]), [`view`] (widget trees from `ultar-ui`), [`app`] (clicks),
//! [`ports`] (device daemon, plan files).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod action;
pub mod app;
pub mod components;
pub mod engine;
pub mod firstboot;
pub mod firstboot_settings;
pub mod i18n;
pub mod model;
pub mod ports;
pub mod secret;
pub mod view;
pub mod wizard;

pub use app::{Exit, InstallerApp};
