//! From the HAL's readings to the bus contract.

use ultar_bus::messages::power::{PowerProfile, PowerSource, PowerState, ThermalLevel};
use ultar_hal::{Power, ThermalZone};

/// Temperature above which the device counts as warm (m°C).
pub const WARM_MC: i32 = 70_000;
/// Temperature above which it counts as hot (m°C).
pub const HOT_MC: i32 = 85_000;

/// The thermal level of the hottest zone.
pub fn thermal(zones: &[ThermalZone]) -> ThermalLevel {
    zones.iter().fold(ThermalLevel::Normal, |level, zone| {
        let this = if zone.is_critical() {
            ThermalLevel::Critical
        } else if zone.millicelsius >= HOT_MC {
            ThermalLevel::Hot
        } else if zone.millicelsius >= WARM_MC {
            ThermalLevel::Warm
        } else {
            ThermalLevel::Normal
        };
        level.max(this)
    })
}

/// The current state as the bus sees it. A device without battery runs from mains.
pub fn read(hal: &dyn Power) -> PowerState {
    let battery = hal.battery().ok().flatten();
    let charger = hal.charger().map(|c| c.is_connected()).unwrap_or(false);
    let external = battery.is_none_or(|b| b.on_external_power()) || charger;
    PowerState {
        source: if external {
            PowerSource::External
        } else {
            PowerSource::Battery
        },
        battery_percent: battery.map(|b| b.percent),
        charging: battery.is_some_and(|b| b.status == ultar_hal::ChargeStatus::Charging),
        profile: PowerProfile::Balanced,
        thermal: thermal(&hal.thermal_zones().unwrap_or_default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_hal_linux::LinuxPower;

    fn zone(mc: i32, critical: Option<i32>) -> ThermalZone {
        ThermalZone {
            name: "z".into(),
            millicelsius: mc,
            critical_millicelsius: critical,
        }
    }

    #[test]
    fn hottest_zone_decides() {
        assert_eq!(thermal(&[]), ThermalLevel::Normal);
        assert_eq!(
            thermal(&[zone(40_000, None), zone(71_000, None)]),
            ThermalLevel::Warm
        );
        assert_eq!(thermal(&[zone(90_000, None)]), ThermalLevel::Hot);
        assert_eq!(
            thermal(&[zone(60_000, Some(50_000))]),
            ThermalLevel::Critical
        );
    }

    #[test]
    fn a_machine_without_battery_is_on_mains() {
        let s = read(&LinuxPower::new("/nonexistent-ultar-root"));
        assert_eq!(s.source, PowerSource::External);
        assert_eq!(s.battery_percent, None);
        assert!(!s.charging);
    }
}
