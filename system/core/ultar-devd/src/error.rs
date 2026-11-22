//! Errors of the device manager's operations.

use crate::fsm::{InvalidTransition, Phase};
use crate::types::DriverKey;
use std::fmt;

/// Why an event or a manual operation was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevdError {
    /// No device is known at this address.
    UnknownDevice(String),
    /// The driver is not a candidate for this device (not in the depot or wrong hardware).
    NotACandidate {
        /// Address of the device.
        device: String,
        /// The requested driver.
        driver: DriverKey,
    },
    /// The driver is quarantined for this device; clear the quarantine first.
    Quarantined {
        /// Address of the device.
        device: String,
        /// The requested driver.
        driver: DriverKey,
    },
    /// The requested driver is already the active one.
    AlreadyActive(DriverKey),
    /// There is no earlier driver to roll back to.
    NothingToRollBack(String),
    /// The operation is not possible in the device's current phase.
    WrongPhase {
        /// Address of the device.
        device: String,
        /// The phase the device is in.
        phase: Phase,
    },
    /// The state machine refused a step (an internal inconsistency; never expected).
    Transition(InvalidTransition),
}

impl DevdError {
    /// Stable message key for the settings panel.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownDevice(_) => "devd.error.unknown-device",
            Self::NotACandidate { .. } => "devd.error.not-a-candidate",
            Self::Quarantined { .. } => "devd.error.quarantined",
            Self::AlreadyActive(_) => "devd.error.already-active",
            Self::NothingToRollBack(_) => "devd.error.nothing-to-roll-back",
            Self::WrongPhase { .. } => "devd.error.wrong-phase",
            Self::Transition(_) => "devd.error.transition",
        }
    }
}

impl fmt::Display for DevdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            Self::UnknownDevice(device) | Self::NothingToRollBack(device) => {
                write!(f, "{code}: {device}")
            }
            Self::NotACandidate { device, driver } | Self::Quarantined { device, driver } => {
                write!(f, "{code}: {device}: {driver}")
            }
            Self::AlreadyActive(driver) => write!(f, "{code}: {driver}"),
            Self::WrongPhase { device, phase } => write!(f, "{code}: {device}: {}", phase.as_str()),
            Self::Transition(inner) => write!(f, "{code}: {inner}"),
        }
    }
}

impl std::error::Error for DevdError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transition(inner) => Some(inner),
            _ => None,
        }
    }
}

impl From<InvalidTransition> for DevdError {
    fn from(inner: InvalidTransition) -> Self {
        Self::Transition(inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsm::Signal;
    use ultar_driverpkg::Version;

    #[test]
    fn codes_and_text() {
        let driver = DriverKey::new("drv", Version::new(1, 0, 0));
        let device = "pci:0".to_string();
        let transition = InvalidTransition {
            from: Phase::Removed,
            signal: Signal::Rescan,
        };
        let cases = [
            (
                DevdError::UnknownDevice(device.clone()),
                "devd.error.unknown-device: pci:0",
            ),
            (
                DevdError::NotACandidate {
                    device: device.clone(),
                    driver: driver.clone(),
                },
                "devd.error.not-a-candidate: pci:0: drv 1.0.0",
            ),
            (
                DevdError::Quarantined {
                    device: device.clone(),
                    driver: driver.clone(),
                },
                "devd.error.quarantined: pci:0: drv 1.0.0",
            ),
            (
                DevdError::AlreadyActive(driver),
                "devd.error.already-active: drv 1.0.0",
            ),
            (
                DevdError::NothingToRollBack(device.clone()),
                "devd.error.nothing-to-roll-back: pci:0",
            ),
            (
                DevdError::WrongPhase {
                    device,
                    phase: Phase::Probing,
                },
                "devd.error.wrong-phase: pci:0: probing",
            ),
            (
                DevdError::from(transition),
                "devd.error.transition: devd.invalid-transition: removed + Rescan",
            ),
        ];
        for (error, text) in cases {
            assert_eq!(error.to_string(), text);
            assert!(text.starts_with(error.code()));
        }
        assert!(std::error::Error::source(&DevdError::from(transition)).is_some());
        assert!(std::error::Error::source(&DevdError::UnknownDevice(String::new())).is_none());
    }
}
