//! The seams to the outside world. The daemon binary and the HAL implement these.

use crate::store::StoredState;
use crate::types::{Capability, CapabilityChange, HealthFailure, LoadFailure};
use std::fmt;
use ultar_driverpkg::{Candidate, DeviceDescriptor};

/// Why a driver could not be loaded for a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadError {
    /// The device disappeared while the driver was loading (hot-unplug).
    DeviceGone,
    /// Loading failed for the given reason.
    Failed(LoadFailure),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeviceGone => f.write_str("devd.load.device-gone"),
            Self::Failed(failure) => write!(f, "devd.load.{}", failure.code()),
        }
    }
}

impl std::error::Error for LoadError {}

/// Loads and unloads drivers (kernel module, firmware, HAL backend).
///
/// The implementation resolves [`Candidate::file`] inside the driver depot of the
/// installed system and must verify the package signature before loading anything.
pub trait DriverLoader {
    /// Loads `driver` for `device`.
    fn load(&mut self, device: &DeviceDescriptor, driver: &Candidate) -> Result<(), LoadError>;

    /// Unloads `driver` from `device` (best effort; also called after a failed health check).
    fn unload(&mut self, device: &DeviceDescriptor, driver: &Candidate);
}

/// The result of a health check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthVerdict {
    /// The device works; these are the capabilities it provides.
    Healthy(Vec<Capability>),
    /// The device does not work with this driver.
    Unhealthy(HealthFailure),
    /// The device disappeared during the check (hot-unplug).
    DeviceGone,
}

/// Checks whether a device works with the driver that was just loaded
/// (picture there? link up? sound test passed?).
pub trait HealthCheck {
    /// Runs the check for `device` driven by `driver`.
    fn check(&mut self, device: &DeviceDescriptor, driver: &Candidate) -> HealthVerdict;
}

/// Why the persistent store could not be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    /// The storage is not available (e.g. data partition not mounted).
    Unavailable,
    /// The stored content cannot be understood.
    Corrupt,
    /// Writing failed.
    WriteFailed,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "devd.store.unavailable",
            Self::Corrupt => "devd.store.corrupt",
            Self::WriteFailed => "devd.store.write-failed",
        })
    }
}

impl std::error::Error for StoreError {}

/// Persists the daemon's decisions (working choices, previous drivers, quarantine) across boots.
pub trait ChoiceStore {
    /// Reads the stored state; an empty state when nothing was stored yet.
    fn load(&mut self) -> Result<StoredState, StoreError>;

    /// Replaces the stored state.
    fn save(&mut self, state: &StoredState) -> Result<(), StoreError>;
}

/// Receives capability announcements (the daemon forwards them to the system bus).
pub trait CapabilitySink {
    /// Publishes one change.
    fn emit(&mut self, change: CapabilityChange);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes() {
        assert_eq!(LoadError::DeviceGone.to_string(), "devd.load.device-gone");
        assert_eq!(
            LoadError::Failed(LoadFailure::InitFailed).to_string(),
            "devd.load.init-failed"
        );
        assert_eq!(
            StoreError::Unavailable.to_string(),
            "devd.store.unavailable"
        );
        assert_eq!(StoreError::Corrupt.to_string(), "devd.store.corrupt");
        assert_eq!(
            StoreError::WriteFailed.to_string(),
            "devd.store.write-failed"
        );
    }
}
