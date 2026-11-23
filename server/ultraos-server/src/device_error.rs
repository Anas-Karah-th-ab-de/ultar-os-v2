//! The typed error of device operations ([`crate::devices`], [`crate::device_moves`]).

use std::fmt;

/// Why a device operation was refused.
#[derive(Debug)]
pub enum DeviceError {
    /// The key is registered to another account.
    Taken,
    /// The key was removed or replaced earlier and stays retired.
    Retired,
    /// The account already has the allowed number of devices.
    Limit,
    /// The account used up its lifetime number of key registrations.
    QuotaExhausted,
    /// No such device in this account.
    Unknown,
    /// The legacy registration cannot be upgraded by this caller.
    LegacyNotEligible,
    /// The device name is too long or contains control characters.
    InvalidName,
    /// No free number was found.
    NumbersExhausted,
    /// Another recovery is pending for the account.
    RecoveryPending,
    /// There is no matching recovery to finish or cancel.
    RecoveryUnknown,
    /// The cool-down of the recovery is not over.
    CooldownActive {
        /// Seconds left.
        retry_after_secs: u64,
    },
    /// The device was added without approval and may not manage other devices yet.
    NotApproved {
        /// Seconds until it may, without approval.
        retry_after_secs: u64,
    },
    /// A registration from before device keys cannot be recovered yet.
    RecoveryLocked {
        /// Seconds until a recovery can be started.
        retry_after_secs: u64,
    },
    /// The database reported an error.
    Db(rusqlite::Error),
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceError::Taken => f.write_str("device key belongs to another account"),
            DeviceError::Retired => f.write_str("device key is retired"),
            DeviceError::Limit => f.write_str("device limit reached"),
            DeviceError::QuotaExhausted => f.write_str("device registration quota used up"),
            DeviceError::Unknown => f.write_str("unknown device"),
            DeviceError::LegacyNotEligible => f.write_str("legacy device not eligible"),
            DeviceError::InvalidName => f.write_str("invalid device name"),
            DeviceError::NumbersExhausted => f.write_str("no free number"),
            DeviceError::RecoveryPending => f.write_str("another recovery is pending"),
            DeviceError::RecoveryUnknown => f.write_str("no matching recovery"),
            DeviceError::CooldownActive { retry_after_secs } => {
                write!(f, "cool-down active for {retry_after_secs} s")
            }
            DeviceError::NotApproved { retry_after_secs } => {
                write!(f, "device not approved, {retry_after_secs} s left")
            }
            DeviceError::RecoveryLocked { retry_after_secs } => {
                write!(f, "recovery locked for {retry_after_secs} s")
            }
            DeviceError::Db(e) => write!(f, "database: {e}"),
        }
    }
}

impl std::error::Error for DeviceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DeviceError::Db(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for DeviceError {
    fn from(e: rusqlite::Error) -> Self {
        DeviceError::Db(e)
    }
}
