//! Power: batteries and chargers (`power_supply` class), temperatures (`thermal` class)
//! and sleep (`/sys/power/state`).
//!
//! Power off and restart are **not** done here: they belong to the service manager,
//! which ends every service in order first (`init.power`). [`LinuxPower::reboot`] and
//! [`LinuxPower::power_off`] therefore answer `NotSupported`.

use crate::sysfs::{entries, number, text};
use std::path::PathBuf;
use ultar_hal::{
    BatteryHealth, BatteryState, ChargeStatus, ChargerKind, ChargerState, HalError, HalResult,
    Power, PowerProfile, RebootTarget, ThermalZone,
};

/// The sleep control below the root (`mem` listed: suspend to memory is offered).
pub const SUSPEND_STATE: &str = "sys/power/state";
/// Most supplies and zones looked at.
const MAX_ENTRIES: usize = 32;

/// The power backend of a kernel with the usual attribute files.
#[derive(Clone, Debug)]
pub struct LinuxPower {
    root: PathBuf,
}

impl LinuxPower {
    /// A backend reading below `root` (`/` on a device).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The root it reads below (`/` on a device).
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    fn supplies(&self) -> Vec<(String, PathBuf)> {
        entries(&self.root.join("sys/class/power_supply"), MAX_ENTRIES)
            .into_iter()
            .filter_map(|p| Some((text(&p.join("type"))?, p)))
            .collect()
    }

    /// Whether the kernel offers suspend to memory.
    pub fn suspend_supported(&self) -> bool {
        text(&self.root.join(SUSPEND_STATE))
            .is_some_and(|states| states.split_whitespace().any(|s| s == "mem"))
    }
}

fn status(word: &str) -> ChargeStatus {
    match word {
        "Charging" => ChargeStatus::Charging,
        "Discharging" => ChargeStatus::Discharging,
        "Full" => ChargeStatus::Full,
        "Not charging" => ChargeStatus::NotCharging,
        _ => ChargeStatus::Unknown,
    }
}

fn health(word: &str) -> BatteryHealth {
    match word {
        "Good" => BatteryHealth::Good,
        "Overheat" => BatteryHealth::Overheat,
        "Cold" => BatteryHealth::Cold,
        "Dead" => BatteryHealth::Dead,
        "Over voltage" => BatteryHealth::OverVoltage,
        _ => BatteryHealth::Unknown,
    }
}

impl Power for LinuxPower {
    fn battery(&self) -> HalResult<Option<BatteryState>> {
        let Some((_, dir)) = self.supplies().into_iter().find(|(t, _)| t == "Battery") else {
            return Ok(None);
        };
        let Some(capacity) = number(&dir.join("capacity")) else {
            return Ok(None);
        };
        Ok(Some(BatteryState {
            percent: capacity.clamp(0, 100) as u8,
            status: status(&text(&dir.join("status")).unwrap_or_default()),
            health: health(&text(&dir.join("health")).unwrap_or_default()),
            temperature_decicelsius: number(&dir.join("temp")).map(|t| t as i32),
            voltage_mv: number(&dir.join("voltage_now")).map(|v| (v / 1000).max(0) as u32),
            current_ma: number(&dir.join("current_now")).map(|c| (c / 1000) as i32),
        }))
    }

    fn charger(&self) -> HalResult<ChargerState> {
        let online = self.supplies().into_iter().find_map(|(kind, dir)| {
            let on = number(&dir.join("online")) == Some(1);
            match kind.as_str() {
                "Mains" if on => Some(ChargerKind::Mains),
                "USB" | "USB_C" | "USB_PD" if on => Some(ChargerKind::Usb),
                "Wireless" if on => Some(ChargerKind::Wireless),
                _ => None,
            }
        });
        Ok(ChargerState {
            kind: online.unwrap_or(ChargerKind::None),
            max_power_mw: None,
        })
    }

    fn thermal_zones(&self) -> HalResult<Vec<ThermalZone>> {
        let zones = entries(&self.root.join("sys/class/thermal"), MAX_ENTRIES)
            .into_iter()
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("thermal_zone"))
            })
            .filter_map(|p| {
                Some(ThermalZone {
                    name: text(&p.join("type")).unwrap_or_default(),
                    millicelsius: number(&p.join("temp"))? as i32,
                    critical_millicelsius: number(&p.join("trip_point_0_temp")).map(|t| t as i32),
                })
            })
            .collect();
        Ok(zones)
    }

    fn power_profile(&self) -> HalResult<PowerProfile> {
        Ok(PowerProfile::Balanced)
    }

    fn set_power_profile(&self, _profile: PowerProfile) -> HalResult<()> {
        Err(HalError::NotSupported)
    }

    fn suspend(&self) -> HalResult<()> {
        if !self.suspend_supported() {
            return Err(HalError::NotSupported);
        }
        std::fs::write(self.root.join(SUSPEND_STATE), "mem").map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                HalError::PermissionDenied
            } else {
                HalError::Io(e.raw_os_error().unwrap_or(0))
            }
        })
    }

    fn reboot(&self, _target: RebootTarget) -> HalResult<()> {
        Err(HalError::NotSupported)
    }

    fn power_off(&self) -> HalResult<()> {
        Err(HalError::NotSupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("hal-power-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn put(root: &std::path::Path, path: &str, value: &str) {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, value).unwrap();
    }

    #[test]
    fn a_laptop_with_battery_charger_and_sleep() {
        let root = tree("laptop");
        put(&root, "sys/class/power_supply/BAT0/type", "Battery\n");
        put(&root, "sys/class/power_supply/BAT0/capacity", "81\n");
        put(&root, "sys/class/power_supply/BAT0/status", "Charging\n");
        put(&root, "sys/class/power_supply/BAT0/health", "Good\n");
        put(
            &root,
            "sys/class/power_supply/BAT0/voltage_now",
            "12400000\n",
        );
        put(&root, "sys/class/power_supply/AC/type", "Mains\n");
        put(&root, "sys/class/power_supply/AC/online", "1\n");
        put(
            &root,
            "sys/class/thermal/thermal_zone0/type",
            "x86_pkg_temp\n",
        );
        put(&root, "sys/class/thermal/thermal_zone0/temp", "45000\n");
        put(&root, "sys/class/thermal/cooling_device0/type", "fan\n");
        put(&root, "sys/power/state", "freeze mem disk\n");
        let p = LinuxPower::new(&root);
        let b = p.battery().unwrap().unwrap();
        assert_eq!(
            (b.percent, b.status, b.health),
            (81, ChargeStatus::Charging, BatteryHealth::Good)
        );
        assert_eq!(b.voltage_mv, Some(12_400));
        assert_eq!(p.charger().unwrap().kind, ChargerKind::Mains);
        let zones = p.thermal_zones().unwrap();
        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0].millicelsius, 45_000);
        assert!(p.suspend_supported());
        p.suspend().unwrap();
        assert_eq!(text(&root.join("sys/power/state")).unwrap(), "mem");
        assert_eq!(p.reboot(RebootTarget::System), Err(HalError::NotSupported));
        assert_eq!(p.power_off(), Err(HalError::NotSupported));
        assert_eq!(
            p.set_power_profile(PowerProfile::Performance),
            Err(HalError::NotSupported)
        );
        assert_eq!(p.power_profile().unwrap(), PowerProfile::Balanced);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_machine_without_battery_or_sleep_says_so() {
        let root = tree("vm");
        put(&root, "sys/class/power_supply/odd/type", "Battery\n");
        put(&root, "sys/power/state", "freeze\n");
        let p = LinuxPower::new(&root);
        assert_eq!(
            p.battery().unwrap(),
            None,
            "a battery without capacity is not reported"
        );
        assert_eq!(p.charger().unwrap().kind, ChargerKind::None);
        assert!(!p.suspend_supported());
        assert_eq!(p.suspend(), Err(HalError::NotSupported));
        let empty = LinuxPower::new(root.join("nothing"));
        assert_eq!(empty.battery().unwrap(), None);
        assert!(empty.thermal_zones().unwrap().is_empty());
        for (word, s) in [
            ("Discharging", ChargeStatus::Discharging),
            ("Full", ChargeStatus::Full),
            ("Not charging", ChargeStatus::NotCharging),
            ("?", ChargeStatus::Unknown),
        ] {
            assert_eq!(status(word), s);
        }
        for (word, h) in [
            ("Overheat", BatteryHealth::Overheat),
            ("Cold", BatteryHealth::Cold),
            ("Dead", BatteryHealth::Dead),
            ("Over voltage", BatteryHealth::OverVoltage),
            ("?", BatteryHealth::Unknown),
        ] {
            assert_eq!(health(word), h);
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
