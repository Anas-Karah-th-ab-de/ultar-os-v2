//! Contracts of the device service (`devd`): what the hardware can do and
//! what its drivers are doing.

use crate::message::{Message, Text, Topic};
use serde::{Deserialize, Serialize};

/// How usable a capability currently is.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Availability {
    /// Hardware present and its driver active.
    Available,
    /// Hardware present but only partly working (fallback driver, no firmware).
    Degraded,
    /// Hardware present but switched off or without a driver.
    Unavailable,
}

/// One thing the device can do, e.g. `modem`, `battery`, `gpu`, `touch`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Capability {
    /// Stable capability identifier as defined by the profile crate.
    pub id: String,
    /// Current availability.
    pub availability: Availability,
    /// Optional explanation for the user when not fully available.
    pub note: Option<Text>,
}

/// `devices.capabilities` – the complete current capability set (retained).
///
/// Settings panels, shell and apps adapt to this instead of asking which
/// edition or device they run on.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Capabilities {
    /// Increases with every change so receivers can ignore stale values.
    pub revision: u64,
    /// All capabilities known right now.
    pub capabilities: Vec<Capability>,
}

impl Capabilities {
    /// Looks a capability up by its identifier.
    pub fn get(&self, id: &str) -> Option<&Capability> {
        self.capabilities
            .iter()
            .find(|capability| capability.id == id)
    }

    /// Whether the capability exists and is at least partly usable.
    pub fn has(&self, id: &str) -> bool {
        self.get(id)
            .is_some_and(|c| c.availability != Availability::Unavailable)
    }
}

impl Message for Capabilities {
    const VERSION: u16 = 1;
}

impl Topic for Capabilities {
    const TOPIC: &'static str = "devices.capabilities";
    const RETAINED: bool = true;
}

/// Life-cycle phase of a driver for one device.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriverPhase {
    /// A matching driver package was found and is being loaded.
    Loading,
    /// The driver is bound and working.
    Active,
    /// Loading or binding failed.
    Failed,
    /// The driver was unloaded or the device went away.
    Removed,
    /// No driver package matches the device.
    Missing,
}

/// `devices.driver_state` – one driver changed its phase.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct DriverState {
    /// Stable device identifier (bus address or hardware id).
    pub device: String,
    /// Name of the driver package, empty when none matches.
    pub driver: String,
    /// New phase.
    pub phase: DriverPhase,
    /// Reason for `Failed`/`Missing`, for display to the user.
    pub reason: Option<Text>,
}

impl Message for DriverState {
    const VERSION: u16 = 1;
}

impl Topic for DriverState {
    const TOPIC: &'static str = "devices.driver_state";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    fn sample() -> Capabilities {
        Capabilities {
            revision: 3,
            capabilities: vec![
                Capability {
                    id: "modem".into(),
                    availability: Availability::Available,
                    note: None,
                },
                Capability {
                    id: "gpu".into(),
                    availability: Availability::Degraded,
                    note: Some(Text::key("devices.gpu.software_fallback")),
                },
                Capability {
                    id: "nfc".into(),
                    availability: Availability::Unavailable,
                    note: None,
                },
            ],
        }
    }

    #[test]
    fn capabilities_round_trip_and_lookup() {
        let caps = sample();
        assert_eq!(
            decode::<Capabilities>(&encode(&caps).unwrap()).unwrap(),
            caps
        );
        assert!(caps.has("modem"));
        assert!(caps.has("gpu"));
        assert!(!caps.has("nfc"));
        assert!(!caps.has("tpm"));
        assert_eq!(
            caps.get("gpu").map(|c| c.availability),
            Some(Availability::Degraded)
        );
        assert!(Capabilities::default().capabilities.is_empty());
    }

    #[test]
    fn driver_state_round_trips() {
        let state = DriverState {
            device: "usb-1-2".into(),
            driver: "rtl8821cu".into(),
            phase: DriverPhase::Failed,
            reason: Some(Text::key("devices.driver.firmware_missing")),
        };
        assert_eq!(
            decode::<DriverState>(&encode(&state).unwrap()).unwrap(),
            state
        );
    }
}
