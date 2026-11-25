//! The Storage page settings the volume service follows: the low-space warning level and
//! whether removable drives mount by themselves. Read at start (`settings.get`) and
//! followed at runtime (`settings.changed`). A value of the wrong type is ignored; one
//! outside its range falls back to the default of the settings schema, never used as it is.
//!
//! The downloads clean-up (`storage.clean_downloads`, `_days`) is the launcher's: it owns
//! the person's file areas, while this service handles untrusted USB file systems and must
//! not reach them. `storage.trim` and `storage.clean_old_updates` are **not** followed
//! here either: nothing in this service can put them into effect
//! (docs/integration/settings-wiring.md says why).

use ultar_bus::messages::settings::SettingValue;

/// Setting: warn when free space of the data partition drops below this percentage.
pub const LOW_SPACE: &str = "storage.low_space_percent";
/// Setting: removable drives mount by themselves (off: they are offered).
pub const AUTOMOUNT: &str = "storage.automount";

/// The keys this service reads at start.
pub const KEYS: [&str; 2] = [LOW_SPACE, AUTOMOUNT];

/// Default of `storage.low_space_percent` (the settings schema).
pub const DEFAULT_LOW_SPACE: u8 = 10;

/// The Storage settings in effect. The defaults are those of the settings schema, so a
/// service that has not heard from the settings service yet behaves like the fresh system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageSettings {
    /// Free-space percentage below which the person is warned (5..=30, step 5).
    pub low_space_percent: u8,
    /// Removable drives mount without asking.
    pub automount: bool,
}

impl Default for StorageSettings {
    fn default() -> Self {
        Self {
            low_space_percent: DEFAULT_LOW_SPACE,
            automount: true,
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
        assert!(s.automount);
    }

    #[test]
    fn good_values_take_effect_and_repeat_changes_are_quiet() {
        let mut s = StorageSettings::default();
        assert!(s.on_changed(LOW_SPACE, &SettingValue::Integer(20)));
        assert_eq!(s.low_space_percent, 20);
        assert!(!s.on_changed(LOW_SPACE, &SettingValue::Integer(20)));
        assert!(s.on_changed(AUTOMOUNT, &SettingValue::Flag(false)));
        assert!(!s.automount);
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
        // The edges themselves are valid.
        assert!(s.on_changed(LOW_SPACE, &SettingValue::Integer(5)));
        assert!(s.on_changed(LOW_SPACE, &SettingValue::Integer(30)));
    }

    #[test]
    fn a_wrong_type_or_another_key_changes_nothing() {
        let mut s = StorageSettings::default();
        assert!(!s.on_changed(LOW_SPACE, &SettingValue::Text("20".into())));
        assert!(!s.on_changed(AUTOMOUNT, &SettingValue::Integer(0)));
        // The downloads clean-up is the launcher's, not this service's.
        assert!(!s.on_changed("storage.clean_downloads", &SettingValue::Flag(true)));
        assert!(!s.on_changed("storage.clean_downloads_days", &SettingValue::Integer(7)));
        assert!(!s.on_changed("storage.trim", &SettingValue::Flag(false)));
        assert!(!s.on_changed("display.scale", &SettingValue::Decimal(1.5)));
        assert_eq!(s, StorageSettings::default());
    }
}
