//! The data types of device operations: lifecycle state, limits, stored rows, list entries and
//! the standing of a key (when it may manage the account's other devices).

use crate::device_error::DeviceError;
use crate::identity::PUBLIC_KEY_BYTES;

/// A registered key may be bound this many times the device limit over an account's lifetime.
pub const QUOTA_FACTOR: i64 = 10;

/// Lifecycle state of a device row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// Registered before device keys existed; has no key yet.
    Legacy,
    /// Registered with a key.
    Active,
    /// Removed by its account; the key is retired.
    Removed,
    /// Its number moved to another key; the key is retired.
    Replaced,
}

impl DeviceState {
    /// The value stored in the database.
    pub fn code(self) -> i64 {
        match self {
            DeviceState::Legacy => 0,
            DeviceState::Active => 1,
            DeviceState::Removed => 2,
            DeviceState::Replaced => 3,
        }
    }

    /// Parses a stored value; unknown values count as removed (fail closed).
    pub fn from_code(code: i64) -> Self {
        match code {
            0 => DeviceState::Legacy,
            1 => DeviceState::Active,
            3 => DeviceState::Replaced,
            _ => DeviceState::Removed,
        }
    }

    /// The name used in the API.
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceState::Legacy => "legacy",
            DeviceState::Active => "active",
            DeviceState::Removed => "removed",
            DeviceState::Replaced => "replaced",
        }
    }

    /// True for devices that are in use (legacy or active).
    pub fn in_use(self) -> bool {
        matches!(self, DeviceState::Legacy | DeviceState::Active)
    }
}

/// Limits of one account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Most devices in use at the same time.
    pub max_devices: i64,
    /// Seconds after which a device added without approval may manage other devices.
    pub cooldown_secs: i64,
}

impl Limits {
    /// Key registrations allowed over the account's lifetime.
    pub fn quota(self) -> i64 {
        self.max_devices.saturating_mul(QUOTA_FACTOR)
    }
}

/// Result of a registration, upgrade or move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    /// Row of the device.
    pub device_row: i64,
    /// Key id of the device.
    pub device_id: String,
    /// Its Ultar number.
    pub number: String,
    /// Token epoch of the device.
    pub epoch: i64,
    /// False when the device was registered already and nothing changed.
    pub created: bool,
    /// Seconds until the device may manage other devices of the account (0 = it may).
    pub manage_in: i64,
}

/// A device row as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRow {
    /// Row id.
    pub row: i64,
    /// Owning account.
    pub account_id: i64,
    /// Key id, or `legacy:<old id>`.
    pub device_id: String,
    /// Public key (`None` for legacy rows).
    pub public_key: Option<[u8; PUBLIC_KEY_BYTES]>,
    /// Lifecycle state.
    pub state: DeviceState,
    /// Token epoch.
    pub epoch: i64,
    /// True when the device was the account's first, or was approved by a managing device.
    pub approved: bool,
    /// Time its current key was bound.
    pub key_since: i64,
}

impl DeviceRow {
    /// Seconds until this device may manage other devices of its account (rename, remove,
    /// approve, read notices, cancel a recovery): 0 for an approved device, otherwise what is
    /// left of `cooldown_secs` since its key was bound. `None` for a device that is not active.
    pub fn manage_in(&self, now: i64, cooldown_secs: i64) -> Option<i64> {
        if self.state != DeviceState::Active {
            return None;
        }
        if self.approved {
            return Some(0);
        }
        Some(
            self.key_since
                .saturating_add(cooldown_secs)
                .saturating_sub(now)
                .max(0),
        )
    }

    /// Fails with [`DeviceError::NotApproved`] unless the device may manage other devices now.
    pub fn require_manager(&self, now: i64, cooldown_secs: i64) -> Result<(), DeviceError> {
        match self.manage_in(now, cooldown_secs) {
            Some(0) => Ok(()),
            Some(wait) => Err(DeviceError::NotApproved {
                retry_after_secs: u64::try_from(wait).unwrap_or(u64::MAX),
            }),
            None => Err(DeviceError::Unknown),
        }
    }
}

/// One entry of the device list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Key id, or `legacy:<old id>`.
    pub device_id: String,
    /// Name given by the user.
    pub name: String,
    /// Number, if the device has one.
    pub number: Option<String>,
    /// Lifecycle state.
    pub state: DeviceState,
    /// Day of registration (Unix seconds, start of day).
    pub added: i64,
    /// Day of last contact (0 = never since keys exist).
    pub last_seen: i64,
    /// Seconds until the device may manage other devices (`None` for legacy rows).
    pub manage_in: Option<i64>,
    /// Public key (`None` for legacy rows); a managing device signs it to approve the device.
    pub public_key: Option<[u8; PUBLIC_KEY_BYTES]>,
}

/// When a newly bound key gets its management rights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Standing {
    /// True: at once.
    pub approved: bool,
    /// Time the key counts as bound from.
    pub key_since: i64,
}
