//! Mouse, touchpad and keyboard settings on their way to the compositor: `pointer.speed`
//! and `pointer.natural_scroll` of the settings service become `Request::ConfigurePointer`,
//! `keyboard.repeat.*` become `Request::ConfigureKeyRepeat` (the display service already
//! holds the compositor's configuration right).

use ultar_bus::messages::settings::SettingValue;
use ultar_surface::proto::{
    Request, MAX_KEY_REPEATS_PER_SECOND, MAX_KEY_REPEAT_DELAY_MS, MAX_POINTER_SPEED,
    MIN_POINTER_SPEED,
};

/// Setting: pointer speed, -10 to 10.
pub const SPEED: &str = "pointer.speed";
/// Setting: reversed scrolling for the mouse.
pub const NATURAL_SCROLL: &str = "pointer.natural_scroll";
/// Setting: a held key repeats.
pub const REPEAT: &str = "keyboard.repeat.enabled";
/// Setting: milliseconds before a held key repeats.
pub const REPEAT_DELAY: &str = "keyboard.repeat.delay_ms";
/// Setting: repeats per second.
pub const REPEAT_RATE: &str = "keyboard.repeat.per_second";

/// The settings panels whose values the compositor follows.
pub const PANELS: [&str; 2] = ["pointer", "keyboard"];

/// What the compositor was last told (or should be told).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputSettings {
    /// Pointer speed.
    pub speed: i8,
    /// Reversed scrolling.
    pub natural_scroll: bool,
    /// A held key repeats.
    pub repeat: bool,
    /// Milliseconds before a held key repeats.
    pub repeat_delay_ms: u16,
    /// Repeats per second.
    pub repeats_per_second: u8,
}

impl Default for InputSettings {
    /// The defaults of the settings schema.
    fn default() -> Self {
        InputSettings {
            speed: 0,
            natural_scroll: false,
            repeat: true,
            repeat_delay_ms: 500,
            repeats_per_second: 25,
        }
    }
}

fn within(value: i64, low: i64, high: i64) -> i64 {
    value.clamp(low, high)
}

impl InputSettings {
    /// Takes a setting that changed; `true` when it was one of these and differs from what
    /// is known (the compositor must be told).
    pub fn on_changed(&mut self, key: &str, value: &SettingValue) -> bool {
        let before = *self;
        match (key, value) {
            (SPEED, SettingValue::Integer(v)) => {
                let limits = (i64::from(MIN_POINTER_SPEED), i64::from(MAX_POINTER_SPEED));
                self.speed = i8::try_from(within(*v, limits.0, limits.1)).unwrap_or(0);
            }
            (NATURAL_SCROLL, SettingValue::Flag(on)) => self.natural_scroll = *on,
            (REPEAT, SettingValue::Flag(on)) => self.repeat = *on,
            (REPEAT_DELAY, SettingValue::Integer(v)) => {
                let most = i64::from(MAX_KEY_REPEAT_DELAY_MS);
                self.repeat_delay_ms = u16::try_from(within(*v, 0, most)).unwrap_or(0);
            }
            (REPEAT_RATE, SettingValue::Integer(v)) => {
                let most = i64::from(MAX_KEY_REPEATS_PER_SECOND);
                self.repeats_per_second = u8::try_from(within(*v, 1, most)).unwrap_or(0);
            }
            _ => {}
        }
        *self != before
    }

    /// The requests that put these values into effect.
    pub fn requests(&self) -> [Request; 2] {
        [
            Request::ConfigurePointer {
                speed: self.speed,
                natural_scroll: self.natural_scroll,
            },
            Request::ConfigureKeyRepeat {
                enabled: self.repeat,
                delay_ms: self.repeat_delay_ms,
                per_second: self.repeats_per_second,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_input_settings_reach_the_compositor() {
        let mut p = InputSettings::default();
        assert!(!p.on_changed("display.mode", &SettingValue::Text("extend".into())));
        assert!(!p.on_changed(SPEED, &SettingValue::Integer(0)));
        assert!(p.on_changed(SPEED, &SettingValue::Integer(7)));
        assert!(p.on_changed(NATURAL_SCROLL, &SettingValue::Flag(true)));
        assert!(!p.on_changed(NATURAL_SCROLL, &SettingValue::Flag(true)));
        assert!(p.on_changed(SPEED, &SettingValue::Integer(99)));
        assert_eq!(
            p.requests()[0],
            Request::ConfigurePointer {
                speed: 10,
                natural_scroll: true
            }
        );
        // A value of the wrong type changes nothing.
        assert!(!p.on_changed(SPEED, &SettingValue::Text("fast".into())));
    }

    #[test]
    fn key_repeat_starts_with_the_schema_defaults_and_follows_the_settings() {
        let mut p = InputSettings::default();
        let repeat = |p: &InputSettings| p.requests()[1].clone();
        assert_eq!(
            repeat(&p),
            Request::ConfigureKeyRepeat {
                enabled: true,
                delay_ms: 500,
                per_second: 25
            }
        );
        assert!(!p.on_changed(REPEAT_DELAY, &SettingValue::Integer(500)));
        assert!(p.on_changed(REPEAT_DELAY, &SettingValue::Integer(300)));
        assert!(p.on_changed(REPEAT_RATE, &SettingValue::Integer(10)));
        assert!(p.on_changed(REPEAT, &SettingValue::Flag(false)));
        assert_eq!(
            repeat(&p),
            Request::ConfigureKeyRepeat {
                enabled: false,
                delay_ms: 300,
                per_second: 10
            }
        );
        assert!(p.on_changed(REPEAT_RATE, &SettingValue::Integer(1_000)));
        assert_eq!(p.repeats_per_second, MAX_KEY_REPEATS_PER_SECOND);
    }
}
