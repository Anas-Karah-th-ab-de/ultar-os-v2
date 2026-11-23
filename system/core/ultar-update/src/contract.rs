//! Bus contract of the update service (service name `update`, contract version 1).
//!
//! Every user-visible state and failure travels as an i18n **key** (`update.state.*`,
//! `update.error.*`), never as text. Long operations (`check`, `download`, `apply`, `offline`)
//! answer at once with "accepted"; their course is reported on the retained topic
//! [`Progress`]. A failed call carries `ErrorCode::Service(code)`; map the code with
//! [`crate::error::UpdateError::key_of_code`].
//!
//! Administrator only: `apply`, `cancel`, `set_policy`, `offline`, `boot_previous`, the
//! metered override of `download`, and replacing a complete staged bundle. Other callers may
//! start a limited number of checks and downloads (`RateLimited` beyond that).

use serde::{Deserialize, Serialize};
use ultar_bus::{Call, Message, Topic};

/// Name of the service on the bus.
pub const SERVICE: &str = "update";

/// What the updater is doing or waiting for.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Nothing to do; the system is up to date as far as known.
    Idle,
    /// Asking the source for a newer release.
    Checking,
    /// A newer release exists and can be downloaded.
    Available,
    /// Downloading (or copying from a stick).
    Downloading,
    /// A download was interrupted and can be continued.
    Paused,
    /// The release is downloaded and verified; it can be installed.
    Staged,
    /// Writing the release to the other slot.
    Applying,
    /// Installed; a restart starts the new version.
    RebootRequired,
    /// The new version is running and is being checked.
    Verifying,
}

impl Phase {
    /// All phases (for catalogue completeness checks).
    pub const ALL: [Phase; 9] = [
        Phase::Idle,
        Phase::Checking,
        Phase::Available,
        Phase::Downloading,
        Phase::Paused,
        Phase::Staged,
        Phase::Applying,
        Phase::RebootRequired,
        Phase::Verifying,
    ];

    /// i18n key of the phase.
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

    /// True while an operation is running (more [`Progress`] events will follow).
    pub fn busy(self) -> bool {
        matches!(self, Phase::Checking | Phase::Downloading | Phase::Applying)
    }
}

/// Topic `update.progress` (retained): the current state of the updater.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Progress {
    /// Phase.
    pub phase: Phase,
    /// Bytes done in the running operation (0 when not applicable).
    pub done: u64,
    /// Bytes in total (0 when not applicable).
    pub total: u64,
    /// The release the phase is about, when one is known.
    pub version: Option<String>,
    /// The release closes a serious hole.
    pub critical: bool,
    /// i18n key of the release notes of that release.
    pub notes_key: Option<String>,
    /// i18n key of the failure that ended the last operation, if it failed.
    pub error_key: Option<String>,
}

impl Message for Progress {
    const VERSION: u16 = 1;
}

impl Topic for Progress {
    const TOPIC: &'static str = "update.progress";
    const RETAINED: bool = true;
}

/// `update.status` – the full state.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Status {}

/// Answer to [`Status`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct StatusReply {
    /// Same content as the last [`Progress`] event.
    pub progress: Progress,
    /// Running version.
    pub current_version: String,
    /// Running slot (`a` or `b`).
    pub slot: String,
    /// Channel of the policy.
    pub channel: String,
    /// Stored rollback floor.
    pub rollback_floor: u64,
    /// The previous version can be started.
    pub previous_startable: bool,
    /// Version in the other slot, when known.
    pub previous_version: Option<String>,
    /// What the network service reported: `Some(true)` metered, `None` unknown.
    pub metered: Option<bool>,
}

/// `update.check` – look for a newer release (accepted; result on [`Progress`]).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Check {}

/// `update.download` – download the newest release (accepted; course on [`Progress`]).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Download {
    /// Use a metered (or unknown) connection for this download. Counts only when the caller
    /// is an administrator; for everyone else the policy's `allow_metered` decides.
    pub allow_metered: bool,
}

/// `update.apply` – install the staged release (admin; accepted; course on [`Progress`]).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Apply {}

/// `update.cancel` – stop the running operation and drop what is pending (admin).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Cancel {}

/// `update.offline` – take the update from a bundle file or folder (admin; accepted).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Offline {
    /// Absolute path of a `.uup` file or of a folder that contains bundles.
    pub path: String,
    /// Accept a release older than the running one (never below the rollback floor).
    pub allow_downgrade: bool,
}

/// `update.boot_previous` – make the previous version start next (admin).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct BootPrevious {}

/// `update.network` – the network service reports whether the connection is metered.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Network {
    /// True when the current connection is metered.
    pub metered: bool,
}

/// `update.history` – the newest history entries.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct History {
    /// Most entries wanted (the service caps this at 200).
    pub limit: u16,
}

/// One history entry on the bus.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct HistoryItem {
    /// Seconds since 1970.
    pub time: u64,
    /// i18n key of what happened (`update.history.*`).
    pub kind_key: String,
    /// Release concerned.
    pub version: Option<String>,
    /// i18n key of the reason, for failures.
    pub detail_key: Option<String>,
}

/// Answer to [`History`], newest entry first.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct HistoryReply {
    /// The entries.
    pub entries: Vec<HistoryItem>,
}

/// `update.get_policy` – read the policy.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct GetPolicy {}

/// The policy on the bus; answer to [`GetPolicy`] and argument of `update.set_policy` (admin).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct PolicyMessage {
    /// Search, download and prepare by itself (a restart is never forced).
    pub automatic: bool,
    /// Daily window as minutes of the day in UTC (`start`, `end`); `None` = any time.
    pub window: Option<(u16, u16)>,
    /// Update channel.
    pub channel: String,
    /// Automatic downloads may use a metered connection.
    pub allow_metered: bool,
    /// Rate limit in KiB/s; 0 = unlimited.
    pub rate_limit_kib: u32,
}

/// Reply of calls that only acknowledge.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Accepted {}

macro_rules! contract {
    ($($call:ty => $method:literal, $reply:ty;)*) => {
        $(
            impl Message for $call {
                const VERSION: u16 = 1;
            }
            impl Call for $call {
                const SERVICE: &'static str = SERVICE;
                const METHOD: &'static str = $method;
                type Reply = $reply;
            }
        )*
        /// All method names of the service (for bus policy files).
        pub const METHODS: &[&str] = &[$($method),*];
    };
}

contract! {
    Status => "status", StatusReply;
    Check => "check", Accepted;
    Download => "download", Accepted;
    Apply => "apply", Accepted;
    Cancel => "cancel", Accepted;
    Offline => "offline", Accepted;
    BootPrevious => "boot_previous", Accepted;
    Network => "network", Accepted;
    History => "history", HistoryReply;
    GetPolicy => "get_policy", PolicyMessage;
    PolicyMessage => "set_policy", Accepted;
}

impl Message for StatusReply {
    const VERSION: u16 = 1;
}
impl Message for HistoryReply {
    const VERSION: u16 = 1;
}
impl Message for Accepted {
    const VERSION: u16 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_keys_are_distinct_and_busy_is_marked() {
        let keys: std::collections::BTreeSet<_> = Phase::ALL.iter().map(|p| p.key()).collect();
        assert_eq!(keys.len(), Phase::ALL.len());
        assert!(Phase::Downloading.busy() && Phase::Applying.busy() && Phase::Checking.busy());
        assert!(!Phase::Staged.busy() && !Phase::RebootRequired.busy());
    }

    #[test]
    fn messages_round_trip_on_the_wire() {
        let progress = Progress {
            phase: Phase::Downloading,
            done: 5,
            total: 10,
            version: Some("0.7.1".into()),
            critical: true,
            notes_key: Some("update.notes.v0_7_1".into()),
            error_key: None,
        };
        let bytes = ultar_bus::encode(&progress).unwrap();
        assert_eq!(ultar_bus::decode::<Progress>(&bytes).unwrap(), progress);
        let policy = PolicyMessage {
            automatic: true,
            window: Some((60, 300)),
            channel: "stable".into(),
            allow_metered: false,
            rate_limit_kib: 256,
        };
        let bytes = ultar_bus::encode(&policy).unwrap();
        assert_eq!(ultar_bus::decode::<PolicyMessage>(&bytes).unwrap(), policy);
        assert_eq!(Progress::TOPIC, "update.progress");
        const { assert!(Progress::RETAINED) };
    }

    #[test]
    fn method_names_are_valid_and_unique() {
        let unique: std::collections::BTreeSet<_> = METHODS.iter().collect();
        assert_eq!(unique.len(), METHODS.len());
        assert_eq!(METHODS.len(), 11);
        for method in METHODS {
            assert!(ultar_bus::is_valid_segment_name(method), "{method}");
        }
        assert_eq!(<Apply as Call>::SERVICE, "update");
        assert_eq!(<PolicyMessage as Call>::METHOD, "set_policy");
    }
}
