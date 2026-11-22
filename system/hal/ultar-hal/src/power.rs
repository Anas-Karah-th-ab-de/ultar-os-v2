//! Power class: battery, charger, temperatures, power profiles and system power state.

use crate::error::HalResult;

/// What the battery is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChargeStatus {
    /// Charging.
    Charging,
    /// Running from the battery.
    Discharging,
    /// Full and on external power.
    Full,
    /// On external power but not charging (charge limit, temperature).
    NotCharging,
    /// The battery does not say.
    Unknown,
}

/// Condition of the battery.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BatteryHealth {
    /// Fine.
    Good,
    /// Too hot.
    Overheat,
    /// Too cold.
    Cold,
    /// Worn out or defective.
    Dead,
    /// Voltage too high.
    OverVoltage,
    /// The battery does not say.
    Unknown,
}

/// State of the battery.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BatteryState {
    /// Charge in percent (0..=100).
    pub percent: u8,
    /// What the battery is doing.
    pub status: ChargeStatus,
    /// Condition.
    pub health: BatteryHealth,
    /// Temperature in tenths of a degree Celsius, if measured.
    pub temperature_decicelsius: Option<i32>,
    /// Voltage in millivolts, if measured.
    pub voltage_mv: Option<u32>,
    /// Current in milliamperes, positive while charging, if measured.
    pub current_ma: Option<i32>,
}

impl BatteryState {
    /// True while energy flows into the battery or it is full on external power.
    pub const fn on_external_power(&self) -> bool {
        matches!(
            self.status,
            ChargeStatus::Charging | ChargeStatus::Full | ChargeStatus::NotCharging
        )
    }

    /// True at or below the given percentage while running from the battery.
    pub const fn is_low(&self, threshold_percent: u8) -> bool {
        self.percent <= threshold_percent && matches!(self.status, ChargeStatus::Discharging)
    }
}

/// Kind of external power.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChargerKind {
    /// Nothing connected.
    None,
    /// Mains adapter.
    Mains,
    /// USB port of a computer.
    Usb,
    /// USB power delivery or another fast charger.
    Fast,
    /// Wireless charging pad.
    Wireless,
}

/// State of external power.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChargerState {
    /// Kind of charger.
    pub kind: ChargerKind,
    /// Power the charger can deliver in milliwatts, if known.
    pub max_power_mw: Option<u32>,
}

impl ChargerState {
    /// True if external power is connected.
    pub const fn is_connected(&self) -> bool {
        !matches!(self.kind, ChargerKind::None)
    }
}

/// One temperature measuring point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThermalZone {
    /// Name of the zone as the platform reports it (technical, not for people).
    pub name: String,
    /// Temperature in thousandths of a degree Celsius.
    pub millicelsius: i32,
    /// Temperature at which the system must shut down, if the zone defines one.
    pub critical_millicelsius: Option<i32>,
}

impl ThermalZone {
    /// True if the zone reached its critical temperature.
    pub fn is_critical(&self) -> bool {
        self.critical_millicelsius
            .is_some_and(|critical| self.millicelsius >= critical)
    }
}

/// Trade-off between speed and energy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PowerProfile {
    /// Lowest energy use.
    PowerSaver,
    /// Balance of speed and energy.
    #[default]
    Balanced,
    /// Highest speed.
    Performance,
}

/// What the device starts after a reboot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RebootTarget {
    /// The normal system.
    #[default]
    System,
    /// The recovery system.
    Recovery,
    /// The boot loader or download mode.
    Bootloader,
    /// The firmware setup.
    Firmware,
}

/// Power hardware of a backend.
pub trait Power: Send + Sync {
    /// State of the battery; `None` if the device has no battery.
    fn battery(&self) -> HalResult<Option<BatteryState>>;
    /// State of external power.
    fn charger(&self) -> HalResult<ChargerState>;
    /// Temperatures.
    fn thermal_zones(&self) -> HalResult<Vec<ThermalZone>>;
    /// Active power profile.
    fn power_profile(&self) -> HalResult<PowerProfile>;
    /// Switches the power profile.
    fn set_power_profile(&self, profile: PowerProfile) -> HalResult<()>;
    /// Suspends the device to memory; returns after it woke up again.
    fn suspend(&self) -> HalResult<()>;
    /// Restarts the device.
    fn reboot(&self, target: RebootTarget) -> HalResult<()>;
    /// Switches the device off.
    fn power_off(&self) -> HalResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battery(percent: u8, status: ChargeStatus) -> BatteryState {
        BatteryState {
            percent,
            status,
            health: BatteryHealth::Good,
            temperature_decicelsius: None,
            voltage_mv: None,
            current_ma: None,
        }
    }

    #[test]
    fn battery_helpers() {
        assert!(battery(10, ChargeStatus::Discharging).is_low(15));
        assert!(battery(15, ChargeStatus::Discharging).is_low(15));
        assert!(!battery(16, ChargeStatus::Discharging).is_low(15));
        assert!(!battery(5, ChargeStatus::Charging).is_low(15));
        assert!(battery(5, ChargeStatus::Charging).on_external_power());
        assert!(battery(100, ChargeStatus::Full).on_external_power());
        assert!(battery(80, ChargeStatus::NotCharging).on_external_power());
        assert!(!battery(80, ChargeStatus::Discharging).on_external_power());
        assert!(!battery(80, ChargeStatus::Unknown).on_external_power());
    }

    #[test]
    fn charger_and_thermal_helpers() {
        let none = ChargerState {
            kind: ChargerKind::None,
            max_power_mw: None,
        };
        let fast = ChargerState {
            kind: ChargerKind::Fast,
            max_power_mw: Some(25_000),
        };
        assert!(!none.is_connected() && fast.is_connected());
        let mut zone = ThermalZone {
            name: "cpu".into(),
            millicelsius: 95_000,
            critical_millicelsius: Some(95_000),
        };
        assert!(zone.is_critical());
        zone.millicelsius = 94_999;
        assert!(!zone.is_critical());
        zone.critical_millicelsius = None;
        zone.millicelsius = 200_000;
        assert!(!zone.is_critical());
        assert_eq!(PowerProfile::default(), PowerProfile::Balanced);
        assert_eq!(RebootTarget::default(), RebootTarget::System);
    }
}
