//! Device profile of Ultar OS (area 3, system core).
//!
//! This crate answers one question for every other part of the system: *what is this
//! device and what can it do?* It replaces the old `ultar-edition` crate. The edition
//! (`phone`, `desktop`, `server`) survives only as a build and service preset; all
//! runtime behaviour (interface mode, visible settings panels, whether an app can be
//! installed) is derived from [`Capabilities`], never from the edition.
//!
//! The crate is pure logic: it never reads hardware paths. The values come from a
//! [`ProfileSource`], whose real implementation lives in the HAL; [`MemorySource`]
//! is the in-memory implementation for tests and tools.
//!
//! Main entry points:
//! - [`detect`] builds a [`DeviceProfile`] from a [`ProfileSource`].
//! - [`resolve_edition`], [`form_factor_from_chassis`], [`detect_form_factor`] are the
//!   pure detection steps.
//! - [`choose_mode`] selects the interface [`Mode`].
//! - [`requirements_met`] and [`visible_panels`] gate apps and settings panels.
//!
//! User-visible names are never produced by the logic; every type exposes a stable
//! message key and [`msg`] carries the Arabic, English and German texts.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

mod capabilities;
mod capability;
mod edition;
mod error;
mod form_factor;
mod mode;
pub mod msg;
mod panels;
mod profile;
mod source;

pub use capabilities::{Capabilities, CpuTopology, DisplayGeometry, SimdLevel};
pub use capability::{Capability, CapabilitySet, CapabilitySetIter};
pub use edition::{resolve_edition, Edition, EDITION_CMDLINE_KEY};
pub use error::ProfileError;
pub use form_factor::{
    detect_form_factor, edition_from_kernel_info, form_factor_from_chassis, DetectInputs,
    FormFactor, TABLET_MIN_DIAGONAL_MM,
};
pub use mode::{choose_mode, Mode, ModePreference};
pub use panels::{panel_rule, requirements_met, visible_panels, PanelId, PanelRule};
pub use profile::{device_id, DeviceProfile, UNKNOWN_MODEL};
pub use source::{detect, MemorySource, ProfileSource};
