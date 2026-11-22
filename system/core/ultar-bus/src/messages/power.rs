//! Contract of the power service: the current energy state of the device.

use crate::message::{Message, Topic};
use serde::{Deserialize, Serialize};

/// Where the device currently draws its energy from.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerSource {
    /// Running from the battery.
    Battery,
    /// Running from a charger or mains supply.
    External,
}

/// Performance profile in effect.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PowerProfile {
    /// Highest performance.
    Performance,
    /// Balance between performance and consumption.
    #[default]
    Balanced,
    /// Lowest consumption.
    Saver,
}

/// Thermal situation of the device.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default, PartialOrd, Ord)]
pub enum ThermalLevel {
    /// No throttling.
    #[default]
    Normal,
    /// Getting warm; background work is slowed down.
    Warm,
    /// Hot; performance is reduced.
    Hot,
    /// Critical; the device may shut down.
    Critical,
}

/// `power.state` – the current energy state (retained).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PowerState {
    /// Current energy source.
    pub source: PowerSource,
    /// Battery charge in percent, `None` on devices without battery.
    pub battery_percent: Option<u8>,
    /// Whether the battery is being charged.
    pub charging: bool,
    /// Profile in effect.
    pub profile: PowerProfile,
    /// Thermal situation.
    pub thermal: ThermalLevel,
}

impl Message for PowerState {
    const VERSION: u16 = 1;
}

impl Topic for PowerState {
    const TOPIC: &'static str = "power.state";
    const RETAINED: bool = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn power_state_round_trips() {
        let phone = PowerState {
            source: PowerSource::Battery,
            battery_percent: Some(81),
            charging: false,
            profile: PowerProfile::Saver,
            thermal: ThermalLevel::Warm,
        };
        let desktop = PowerState {
            source: PowerSource::External,
            battery_percent: None,
            charging: false,
            profile: PowerProfile::default(),
            thermal: ThermalLevel::default(),
        };
        for state in [phone, desktop] {
            assert_eq!(
                decode::<PowerState>(&encode(&state).unwrap()).unwrap(),
                state
            );
        }
        assert!(ThermalLevel::Critical > ThermalLevel::Hot);
        assert_eq!(PowerProfile::default(), PowerProfile::Balanced);
    }
}
