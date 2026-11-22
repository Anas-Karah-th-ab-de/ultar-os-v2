//! Mock power hardware.

use super::cell::MockCell;
use crate::error::{HalError, HalResult};
use crate::power::{
    BatteryHealth, BatteryState, ChargeStatus, ChargerKind, ChargerState, Power, PowerProfile,
    RebootTarget, ThermalZone,
};

/// A power state change a test asked the mock for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PowerAction {
    /// [`Power::suspend`] was called.
    Suspend,
    /// [`Power::reboot`] was called.
    Reboot(RebootTarget),
    /// [`Power::power_off`] was called.
    PowerOff,
}

#[derive(Debug)]
struct PowerState {
    battery: Option<BatteryState>,
    charger: ChargerState,
    zones: Vec<ThermalZone>,
    profile: PowerProfile,
    actions: Vec<PowerAction>,
}

impl Default for PowerState {
    fn default() -> Self {
        PowerState {
            battery: None,
            charger: ChargerState {
                kind: ChargerKind::Mains,
                max_power_mw: None,
            },
            zones: Vec::new(),
            profile: PowerProfile::Balanced,
            actions: Vec::new(),
        }
    }
}

/// Scriptable power hardware. Starts as a mains-powered device without battery.
#[derive(Debug, Default)]
pub struct MockPower {
    cell: MockCell<PowerState>,
}

impl MockPower {
    /// Battery state used by the presets: discharging and healthy at the given charge.
    pub fn discharging_battery(percent: u8) -> BatteryState {
        BatteryState {
            percent: percent.min(100),
            status: ChargeStatus::Discharging,
            health: BatteryHealth::Good,
            temperature_decicelsius: Some(285),
            voltage_mv: Some(3_850),
            current_ma: Some(-420),
        }
    }

    /// Makes the next trait call fail with `error`.
    pub fn fail_next(&self, error: HalError) {
        self.cell.fail_next(error);
    }

    /// Sets or removes the battery.
    pub fn set_battery(&self, battery: Option<BatteryState>) {
        self.cell.state().battery = battery;
    }

    /// Sets the external power state.
    pub fn set_charger(&self, charger: ChargerState) {
        self.cell.state().charger = charger;
    }

    /// Sets the temperature of a zone, creating the zone if needed.
    pub fn set_zone(&self, name: &str, millicelsius: i32, critical_millicelsius: Option<i32>) {
        let mut state = self.cell.state();
        state.zones.retain(|zone| zone.name != name);
        state.zones.push(ThermalZone {
            name: name.to_string(),
            millicelsius,
            critical_millicelsius,
        });
    }

    /// Power state changes requested so far, in order.
    pub fn actions(&self) -> Vec<PowerAction> {
        self.cell.state().actions.clone()
    }
}

impl Power for MockPower {
    fn battery(&self) -> HalResult<Option<BatteryState>> {
        self.cell.call(|state| Ok(state.battery))
    }

    fn charger(&self) -> HalResult<ChargerState> {
        self.cell.call(|state| Ok(state.charger))
    }

    fn thermal_zones(&self) -> HalResult<Vec<ThermalZone>> {
        self.cell.call(|state| Ok(state.zones.clone()))
    }

    fn power_profile(&self) -> HalResult<PowerProfile> {
        self.cell.call(|state| Ok(state.profile))
    }

    fn set_power_profile(&self, profile: PowerProfile) -> HalResult<()> {
        self.cell.call(|state| {
            state.profile = profile;
            Ok(())
        })
    }

    fn suspend(&self) -> HalResult<()> {
        self.cell.call(|state| {
            state.actions.push(PowerAction::Suspend);
            Ok(())
        })
    }

    fn reboot(&self, target: RebootTarget) -> HalResult<()> {
        self.cell.call(|state| {
            state.actions.push(PowerAction::Reboot(target));
            Ok(())
        })
    }

    fn power_off(&self) -> HalResult<()> {
        self.cell.call(|state| {
            state.actions.push(PowerAction::PowerOff);
            Ok(())
        })
    }
}
