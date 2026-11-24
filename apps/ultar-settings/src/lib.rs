//! `ultar-settings` – the Settings app of Ultar OS (area 6 "apps", plan §3.0; DESKTOP-SPEC §8,
//! §12.1, §13; boards `SettingsDisplay`, `SettingsDisplayLight`, `SettingsDrivers`, `Account`).
//!
//! Surface = app, state and rules = service (APP-RULES §1): this crate only shows what the
//! services offer and sends what the user chose:
//! - `settingsd` (`settings.*`): every stored setting, the confirm-or-revert of risky display
//!   changes (the "keep?" question itself is asked by the system dialog service, which follows
//!   `settings.pending`);
//! - the display service (`display.*`): names, modes and capabilities of the connected
//!   displays, "show display numbers", "look for displays";
//! - the device daemon (`devices.*`): drivers, alternatives, rollback, rescan (self-test);
//! - the updater (`update.*`): firmware/system update check and update from a drive;
//! - the account service (`account.*`): the Ultar account.
//!
//! Layers: [`action`] (what a click means), [`state`] (pure data), [`update`] (click → state
//! change + service calls, no I/O of its own), [`view`] (widget trees from `ultar-ui` only),
//! [`service`] (ports as traits, bus implementations), [`app`] (the loop glue).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod action;
pub mod app;
pub mod areas;
pub mod error;
pub mod i18n;
pub mod service;
pub mod state;
pub mod update;
pub mod view;
pub mod wired;

pub use app::App;
pub use error::AppError;
