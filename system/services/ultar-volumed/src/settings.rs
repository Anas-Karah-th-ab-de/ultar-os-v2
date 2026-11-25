//! The Storage page settings the volume service follows: the low-space warning level,
//! whether removable drives mount by themselves, and the daily clean-up of the downloads
//! folder. Read at start (`settings.get`) and followed at runtime (`settings.changed`).
//! A value of the wrong type is ignored; one outside its range falls back to the default
//! of the settings schema, never used as it is.
//!
//! `storage.trim` and `storage.clean_old_updates` are **not** followed here: nothing in
//! this service can put them into effect (docs/integration/settings-wiring.md says why).

use ultar_bus::messages::settings::SettingValue;

/// Setting: warn when free space of the data partition drops below this percentage.
pub const LOW_SPACE: &str = "storage.low_space_percent";
/// Setting: removable drives mount by themselves (off: they are offered).
pub const AUTOMOUNT: &str = "storage.automount";
/// Setting: clean up the downloads folder daily.
pub const CLEAN_DOWNLOADS: &str = "storage.clean_downloads";
/// Setting: days before a download is deleted.
pub const CLEAN_DAYS: &str = "storage.clean_downloads_days";

/// The keys this service reads at start.
pub const KEYS: [&str; 4] = [LOW_SPACE, AUTOMOUNT, CLEAN_DOWNLOADS, CLEAN_DAYS];

/// Default of `storage.low_space_percent` (the settings schema).
pub const DEFAULT_LOW_SPACE: u8 = 10;
/// Default of `storage.clean_downloads_days`.
pub const DEFAULT_CLEAN_DAYS: u16 = 30;

/// The Storage settings in effect. The defaults are those of the settings schema, so a
/// service that has not heard from the settings service yet behaves like the fresh system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageSettings {
    /// Free-space percentage below which the person is warned (5..=30, step 5).
    pub low_space_percent: u8,
    /// Removable drives mount without asking.
    pub automount: bool,
    /// The downloads folder is cleaned up daily.
    pub clean_downloads: bool,
    /// Days a download is kept before the clean-up removes it (1..=365).
    pub clean_downloads_days: u16,
}

impl Default for StorageSettings {
    fn default() -> Self {
        Self {
            low_space_percent: DEFAULT_LOW_SPACE,
            automount: true,
            clean_downloads: false,
            clean_downloads_days: DEFAULT_CLEAN_DAYS,
        }
    }
}

/// A whole number the schema accepts: within `min..=max` and on a step from `min`.
fn whole(value: i64, min: i64, max: i64, step: i64) -> Option<i64> {
    (value >= min && value <= max && (value - min) % step == 0).then_some(value)
}

impl StorageSettings {
    /// Takes a setting that changed; `true` when it was one of these and the value in
    /// effect differs from before. A wrong type changes nothing; an out-of-range number
    /// falls back to the schema default.
    pub fn on_changed(&mut self, key: &str, value: &SettingValue) -> bool {
        let before = *self;
        match (key, value) {
            (LOW_SPACE, SettingValue::Integer(v)) => {
                self.low_space_percent = whole(*v, 5, 30, 5)
                    .and_then(|n| u8::try_from(n).ok())
                    .unwrap_or(DEFAULT_LOW_SPACE);
            }
            (AUTOMOUNT, SettingValue::Flag(on)) => self.automount = *on,
            (CLEAN_DOWNLOADS, SettingValue::Flag(on)) => self.clean_downloads = *on,
            (CLEAN_DAYS, SettingValue::Integer(v)) => {
                self.clean_downloads_days = whole(*v, 1, 365, 1)
                    .and_then(|n| u16::try_from(n).ok())
                    .unwrap_or(DEFAULT_CLEAN_DAYS);
            }
            _ => {}
        }
        *self != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_schema_defaults() {
        let s = StorageSettings::default();
        assert_eq!(s.low_space_percent, 10);
        assert!(s.automount && !s.clean_downloads);
        assert_eq!(s.clean_downloads_days, 30);
    }

    #[test]
    fn good_values_take_effect_and_repeat_changes_are_quiet() {
        let mut s = StorageSettings::default();
        assert!(s.on_changed(LOW_SPACE, &SettingValue::Integer(20)));
        assert_eq!(s.low_space_percent, 20);
        assert!(!s.on_changed(LOW_SPACE, &SettingValue::Integer(20)));
        assert!(s.on_changed(AUTOMOUNT, &SettingValue::Flag(false)));
        assert!(s.on_changed(CLEAN_DOWNLOADS, &SettingValue::Flag(true)));
        assert!(s.on_changed(CLEAN_DAYS, &SettingValue::Integer(7)));
        assert_eq!(s.clean_downloads_days, 7);
        assert!(!s.automount && s.clean_downloads);
    }

    #[test]
    fn out_of_range_numbers_fall_back_to_the_default() {
        let mut s = StorageSettings::default();
        assert!(s.on_changed(LOW_SPACE, &SettingValue::Integer(20)));
        // Above, below and off the step of 5: back to 10, not clamped to the edge.
        for bad in [31, 4, 0, -5, 7, 100] {
            assert!(s.on_changed(LOW_SPACE, &SettingValue::Integer(bad)));
            assert_eq!(s.low_space_percent, DEFAULT_LOW_SPACE, "{bad}");
            assert!(s.on_changed(LOW_SPACE, &SettingValue::Integer(20)));
        }
        assert!(s.on_changed(CLEAN_DAYS, &SettingValue::Integer(7)));
        for bad in [0, 366, -1, 1000] {
            assert!(s.on_changed(CLEAN_DAYS, &SettingValue::Integer(bad)));
            assert_eq!(s.clean_downloads_days, DEFAULT_CLEAN_DAYS, "{bad}");
            assert!(s.on_changed(CLEAN_DAYS, &SettingValue::Integer(7)));
        }
        // The edges themselves are valid.
        assert!(s.on_changed(LOW_SPACE, &SettingValue::Integer(5)));
        assert!(s.on_changed(LOW_SPACE, &SettingValue::Integer(30)));
        assert!(s.on_changed(CLEAN_DAYS, &SettingValue::Integer(1)));
        assert!(s.on_changed(CLEAN_DAYS, &SettingValue::Integer(365)));
    }

    #[test]
    fn a_wrong_type_or_another_key_changes_nothing() {
        let mut s = StorageSettings::default();
        assert!(!s.on_changed(LOW_SPACE, &SettingValue::Text("20".into())));
        assert!(!s.on_changed(AUTOMOUNT, &SettingValue::Integer(0)));
        assert!(!s.on_changed(CLEAN_DAYS, &SettingValue::Flag(true)));
        assert!(!s.on_changed("storage.trim", &SettingValue::Flag(false)));
        assert!(!s.on_changed("display.scale", &SettingValue::Decimal(1.5)));
        assert_eq!(s, StorageSettings::default());
    }
}
