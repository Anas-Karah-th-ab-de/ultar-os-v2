//! App-Builder (area 6, apps): the developer app that turns an app project into a signed
//! `.uap` and installs it on the same device for testing.
//!
//! A project folder holds `ultar-app.toml` (the manifest of docs/APP-RULES.md §3, plus `arch`)
//! and `payload/` – the built files laid out as they are installed. App-Builder
//!
//! 1. **checks** the manifest and the payload ([`project`]),
//! 2. **builds the package**: packs `payload/` with the manifest through `ultar-pkg` and signs it
//!    with the developer's own key ([`devkey`], kept in the app's data area), then verifies the
//!    result against that key before it keeps it ([`package`]),
//! 3. **installs it for testing** through the launcher (`launcher.install`), which checks the
//!    signature again against the device's trusted keys.
//!
//! Compiling the program is not part of the app (apps start no system programs,
//! APP-RULES §4): it is done with the developer's toolchain, the result goes into `payload/`.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod devkey;
pub mod error;
pub mod i18n;
pub mod package;
pub mod project;
pub mod state;
pub mod view;

pub use error::{AppError, Issue};
pub use state::State;
