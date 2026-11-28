//! The messages of the updater's contract this app uses (`update.check`, `update.download`,
//! `update.apply`, `update.offline`, topic `update.progress`).
//!
//! The contract lives in `system/core/ultar-update/src/contract.rs`, which an app may not
//! depend on (`ultar-check layers`: apps → `ultar-bus` only). These types are field-for-field
//! copies with the same versions, so they encode identically (postcard, field order);
//! `tests/update_contract.rs` holds the reference bytes. Integration: move the contract into
//! `ultar-bus/src/messages/` and delete this file (docs/integration/settings-installer.md).

use serde::{Deserialize, Serialize};
use ultar_bus::{Call, Message, Topic};

/// Phase of the updater (same order as `ultar_update::contract::Phase`).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Up to date.
    Idle,
    /// Asking the source.
    Checking,
    /// A newer release exists.
    Available,
    /// Downloading.
    Downloading,
    /// Download interrupted.
    Paused,
    /// Downloaded and verified.
    Staged,
    /// Writing to the other slot.
    Applying,
    /// A restart starts the new version.
    RebootRequired,
    /// The new version is being checked.
    Verifying,
}

impl Phase {
    /// i18n key of the phase (`update.state.*`, catalogs of the updater).
    pub fn key(self) -> &'static str {
        match self {
            Phase::Idle => "update.state.idle",
            Phase::Checking => "update.state.checking",
            Phase::Available => "update.state.available",
            Phase::Downloading => "update.state.downloading",
            Phase::Paused => "update.state.paused",
            Phase::Staged => "update.state.staged",
            Phase::Applying => "update.state.applying",
            Phase::RebootRequired => "update.state.reboot_required",
            Phase::Verifying => "update.state.verifying",
        }
    }
}

/// `update.progress` (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Progress {
    /// Phase.
    pub phase: Phase,
    /// Bytes done.
    pub done: u64,
    /// Bytes in all.
    pub total: u64,
    /// Version of the release concerned.
    pub version: Option<String>,
    /// Security relevant.
    pub critical: bool,
    /// Release notes key.
    pub notes_key: Option<String>,
    /// Last failure (`update.error.*`).
    pub error_key: Option<String>,
}

impl Message for Progress {
    const VERSION: u16 = 1;
}

impl Topic for Progress {
    const TOPIC: &'static str = "update.progress";
    const RETAINED: bool = true;
}

/// `update.check`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Check {}

/// `update.download` – download the newest release.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Download {
    /// Use a metered (or unknown) connection; counts only for an administrator of the updater.
    pub allow_metered: bool,
}

/// `update.apply` – install the downloaded release into the other slot.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Apply {}

/// `update.offline` – an update from a drive or folder.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Offline {
    /// Path of the bundle.
    pub path: String,
    /// Allow an older version.
    pub allow_downgrade: bool,
}

/// "Accepted" answer of the long operations.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Accepted {}

impl Message for Check {
    const VERSION: u16 = 1;
}
impl Message for Offline {
    const VERSION: u16 = 1;
}
impl Message for Download {
    const VERSION: u16 = 1;
}
impl Message for Apply {
    const VERSION: u16 = 1;
}
impl Call for Download {
    const SERVICE: &'static str = "update";
    const METHOD: &'static str = "download";
    type Reply = Accepted;
}
impl Call for Apply {
    const SERVICE: &'static str = "update";
    const METHOD: &'static str = "apply";
    type Reply = Accepted;
}
impl Message for Accepted {
    const VERSION: u16 = 1;
}
impl Call for Check {
    const SERVICE: &'static str = "update";
    const METHOD: &'static str = "check";
    type Reply = Accepted;
}
impl Call for Offline {
    const SERVICE: &'static str = "update";
    const METHOD: &'static str = "offline";
    type Reply = Accepted;
}
