//! Mouse, touchpad and keyboard settings on their way to the compositor: the `pointer.*`
//! settings and the pointer parts of `accessibility.*` become `Request::ConfigurePointer`,
//! `keyboard.repeat.*` become `Request::ConfigureKeyRepeat`, `keyboard.switch_shortcut` and
//! `keyboard.per_window` become `Request::ConfigureKeyboard` (the display service already
//! holds the compositor's configuration right).

use ultar_bus::messages::settings::SettingValue;
use ultar_surface::proto::{
    PointerSize, Request, SwitchShortcut, DEFAULT_DOUBLE_CLICK_MS, MAX_DOUBLE_CLICK_MS,
    MAX_KEY_REPEATS_PER_SECOND, MAX_KEY_REPEAT_DELAY_MS, MAX_POINTER_SPEED, MIN_DOUBLE_CLICK_MS,
    MIN_POINTER_SPEED,
};

/// Setting: pointer speed, -10 to 10.
pub const SPEED: &str = "pointer.speed";
/// Setting: reversed scrolling for the mouse.
pub const NATURAL_SCROLL: &str = "pointer.natural_scroll";
/// Setting: which button selects and clicks (`left` or `right`).
pub const PRIMARY_BUTTON: &str = "pointer.primary_button";
/// Setting: faster movements travel farther.
pub const ACCELERATION: &str = "pointer.acceleration";
/// Setting: edge of the pointer image (`normal`, `large`, `larger`).
pub const SIZE: &str = "pointer.size";
/// Setting: a large pointer, whatever `pointer.size` says.
pub const LARGE_POINTER: &str = "accessibility.large_pointer";
/// Setting: milliseconds between two clicks of a double click.
pub const DOUBLE_CLICK_MS: &str = "accessibility.double_click_ms";
/// Setting: a held key repeats.
pub const REPEAT: &str = "keyboard.repeat.enabled";
/// Setting: milliseconds before a held key repeats.
pub const REPEAT_DELAY: &str = "keyboard.repeat.delay_ms";
/// Setting: repeats per second.
pub const REPEAT_RATE: &str = "keyboard.repeat.per_second";
/// Setting: the keys that switch the input language.
pub const SWITCH_SHORTCUT: &str = "keyboard.switch_shortcut";
/// Setting: every window remembers its own input language.
pub const PER_WINDOW: &str = "keyboard.per_window";

/// The settings panels whose values the compositor follows.
pub const PANELS: [&str; 3] = ["pointer", "keyboard", "accessibility"];

/// What the compositor was last told (or should be told).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputSettings {
    /// Pointer speed.
    pub speed: i8,
    /// Reversed scrolling.
    pub natural_scroll: bool,
    /// The right button is the primary one.
    pub swap_buttons: bool,
    /// Faster movements travel farther.
    pub acceleration: bool,
    /// Edge of the pointer image (`pointer.size`).
    pub size: PointerSize,
    /// `accessibility.large_pointer`: the pointer is at least [`PointerSize::Large`].
    pub large_pointer: bool,
    /// Milliseconds between two clicks of a double click.
    pub double_click_ms: u16,
    /// A held key repeats.
    pub repeat: bool,
    /// Milliseconds before a held key repeats.
    pub repeat_delay_ms: u16,
    /// Repeats per second.
    pub repeats_per_second: u8,
    /// The keys that switch the input language.
    pub switch_shortcut: SwitchShortcut,
    /// Every window remembers its own input language.
    pub per_window: bool,
}

impl Default for InputSettings {
    /// The defaults of the settings schema.
    fn default() -> Self {
        InputSettings {
            speed: 0,
            natural_scroll: false,
            swap_buttons: false,
            acceleration: true,
            size: PointerSize::Normal,
            large_pointer: false,
            double_click_ms: DEFAULT_DOUBLE_CLICK_MS,
            repeat: true,
            repeat_delay_ms: 500,
            repeats_per_second: 25,
            switch_shortcut: SwitchShortcut::SuperSpace,
            per_window: false,
        }
    }
}

fn within(value: i64, low: i64, high: i64) -> i64 {
    value.clamp(low, high)
}

/// The pointer size a word of the setting names; anything else is the default.
fn size_word(t: &str) -> PointerSize {
    match t {
        "large" => PointerSize::Large,
        "larger" => PointerSize::Larger,
        _ => PointerSize::Normal,
    }
}

/// The switch combination a word of the setting names; anything else is the default.
fn shortcut_word(t: &str) -> SwitchShortcut {
    match t {
        "alt_shift" => SwitchShortcut::AltShift,
        "ctrl_shift" => SwitchShortcut::CtrlShift,
        _ => SwitchShortcut::SuperSpace,
    }
}

impl InputSettings {
    /// Takes a setting that changed; `true` when it was one of these and differs from what
    /// is known (the compositor must be told). Values of the wrong type change nothing;
    /// words and numbers outside their schema fall back to the default or the nearest end.
    pub fn on_changed(&mut self, key: &str, value: &SettingValue) -> bool {
        let before = *self;
        match (key, value) {
            (SPEED, SettingValue::Integer(v)) => {
                let limits = (i64::from(MIN_POINTER_SPEED), i64::from(MAX_POINTER_SPEED));
                self.speed = i8::try_from(within(*v, limits.0, limits.1)).unwrap_or(0);
            }
            (NATURAL_SCROLL, SettingValue::Flag(on)) => self.natural_scroll = *on,
            (PRIMARY_BUTTON, SettingValue::Text(t)) => self.swap_buttons = t == "right",
            (ACCELERATION, SettingValue::Flag(on)) => self.acceleration = *on,
            (SIZE, SettingValue::Text(t)) => self.size = size_word(t),
            (LARGE_POINTER, SettingValue::Flag(on)) => self.large_pointer = *on,
            (DOUBLE_CLICK_MS, SettingValue::Integer(v)) => {
                let limits = (
                    i64::from(MIN_DOUBLE_CLICK_MS),
                    i64::from(MAX_DOUBLE_CLICK_MS),
                );
                self.double_click_ms = u16::try_from(within(*v, limits.0, limits.1))
                    .unwrap_or(DEFAULT_DOUBLE_CLICK_MS);
            }
            (REPEAT, SettingValue::Flag(on)) => self.repeat = *on,
            (REPEAT_DELAY, SettingValue::Integer(v)) => {
                let most = i64::from(MAX_KEY_REPEAT_DELAY_MS);
                self.repeat_delay_ms = u16::try_from(within(*v, 0, most)).unwrap_or(0);
            }
            (REPEAT_RATE, SettingValue::Integer(v)) => {
                let most = i64::from(MAX_KEY_REPEATS_PER_SECOND);
                self.repeats_per_second = u8::try_from(within(*v, 1, most)).unwrap_or(0);
            }
            (SWITCH_SHORTCUT, SettingValue::Text(t)) => self.switch_shortcut = shortcut_word(t),
            (PER_WINDOW, SettingValue::Flag(on)) => self.per_window = *on,
            _ => {}
        }
        *self != before
    }

    /// The pointer size in effect: a large pointer never shrinks below `large`.
    pub fn effective_size(&self) -> PointerSize {
        if self.large_pointer {
            self.size.at_least(PointerSize::Large)
        } else {
            self.size
        }
    }

    /// The requests that put these values into effect.
    pub fn requests(&self) -> [Request; 3] {
        [
            Request::ConfigurePointer {
                speed: self.speed,
                natural_scroll: self.natural_scroll,
                swap_buttons: self.swap_buttons,
                acceleration: self.acceleration,
                size: self.effective_size(),
                double_click_ms: self.double_click_ms,
            },
            Request::ConfigureKeyRepeat {
                enabled: self.repeat,
                delay_ms: self.repeat_delay_ms,
                per_second: self.repeats_per_second,
            },
            Request::ConfigureKeyboard {
                switch_shortcut: self.switch_shortcut,
                per_window: self.per_window,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer(p: &InputSettings) -> Request {
        p.requests()[0].clone()
    }

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
            pointer(&p),
            Request::ConfigurePointer {
                speed: 10,
                natural_scroll: true,
                swap_buttons: false,
                acceleration: true,
                size: PointerSize::Normal,
                double_click_ms: 400,
            }
        );
        // A value of the wrong type changes nothing.
        assert!(!p.on_changed(SPEED, &SettingValue::Text("fast".into())));
    }

    #[test]
    fn button_swap_acceleration_and_size_follow_the_settings() {
        let mut p = InputSettings::default();
        assert!(p.on_changed(PRIMARY_BUTTON, &SettingValue::Text("right".into())));
        assert!(p.on_changed(ACCELERATION, &SettingValue::Flag(false)));
        assert!(p.on_changed(SIZE, &SettingValue::Text("larger".into())));
        assert_eq!(
            pointer(&p),
            Request::ConfigurePointer {
                speed: 0,
                natural_scroll: false,
                swap_buttons: true,
                acceleration: false,
                size: PointerSize::Larger,
                double_click_ms: 400,
            }
        );
        // Back to the defaults, one setting at a time.
        assert!(p.on_changed(PRIMARY_BUTTON, &SettingValue::Text("left".into())));
        assert!(p.on_changed(ACCELERATION, &SettingValue::Flag(true)));
        assert!(p.on_changed(SIZE, &SettingValue::Text("normal".into())));
        assert_eq!(pointer(&p), pointer(&InputSettings::default()));
        // An unknown word falls back to the default; a wrong type changes nothing.
        assert!(p.on_changed(SIZE, &SettingValue::Text("large".into())));
        assert!(p.on_changed(SIZE, &SettingValue::Text("huge".into())));
        assert_eq!(p.size, PointerSize::Normal);
        assert!(!p.on_changed(PRIMARY_BUTTON, &SettingValue::Flag(true)));
        assert!(!p.on_changed(SIZE, &SettingValue::Integer(3)));
        assert!(!p.on_changed(ACCELERATION, &SettingValue::Text("on".into())));
    }

    #[test]
    fn a_large_pointer_forces_at_least_the_large_size() {
        let mut p = InputSettings::default();
        assert!(p.on_changed(LARGE_POINTER, &SettingValue::Flag(true)));
        assert_eq!(p.effective_size(), PointerSize::Large);
        assert!(p.on_changed(SIZE, &SettingValue::Text("larger".into())));
        assert_eq!(p.effective_size(), PointerSize::Larger, "never shrinks");
        assert!(p.on_changed(SIZE, &SettingValue::Text("normal".into())));
        assert_eq!(p.effective_size(), PointerSize::Large);
        assert!(p.on_changed(LARGE_POINTER, &SettingValue::Flag(false)));
        assert_eq!(p.effective_size(), PointerSize::Normal);
        assert!(!p.on_changed(LARGE_POINTER, &SettingValue::Integer(1)));
    }

    #[test]
    fn the_double_click_time_stays_within_its_range() {
        let mut p = InputSettings::default();
        assert!(!p.on_changed(DOUBLE_CLICK_MS, &SettingValue::Integer(400)));
        assert!(p.on_changed(DOUBLE_CLICK_MS, &SettingValue::Integer(700)));
        assert_eq!(p.double_click_ms, 700);
        assert!(p.on_changed(DOUBLE_CLICK_MS, &SettingValue::Integer(5000)));
        assert_eq!(p.double_click_ms, MAX_DOUBLE_CLICK_MS);
        assert!(p.on_changed(DOUBLE_CLICK_MS, &SettingValue::Integer(0)));
        assert_eq!(p.double_click_ms, MIN_DOUBLE_CLICK_MS);
        assert!(!p.on_changed(DOUBLE_CLICK_MS, &SettingValue::Flag(true)));
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

    #[test]
    fn the_switch_shortcut_and_per_window_follow_the_settings() {
        let mut p = InputSettings::default();
        let keyboard = |p: &InputSettings| p.requests()[2].clone();
        assert_eq!(
            keyboard(&p),
            Request::ConfigureKeyboard {
                switch_shortcut: SwitchShortcut::SuperSpace,
                per_window: false,
            }
        );
        assert!(!p.on_changed(SWITCH_SHORTCUT, &SettingValue::Text("super_space".into())));
        assert!(p.on_changed(SWITCH_SHORTCUT, &SettingValue::Text("alt_shift".into())));
        assert!(p.on_changed(PER_WINDOW, &SettingValue::Flag(true)));
        assert_eq!(
            keyboard(&p),
            Request::ConfigureKeyboard {
                switch_shortcut: SwitchShortcut::AltShift,
                per_window: true,
            }
        );
        assert!(p.on_changed(SWITCH_SHORTCUT, &SettingValue::Text("ctrl_shift".into())));
        assert_eq!(p.switch_shortcut, SwitchShortcut::CtrlShift);
        // An unknown word falls back to the default; a wrong type changes nothing.
        assert!(p.on_changed(SWITCH_SHORTCUT, &SettingValue::Text("super_l".into())));
        assert_eq!(p.switch_shortcut, SwitchShortcut::SuperSpace);
        assert!(!p.on_changed(SWITCH_SHORTCUT, &SettingValue::Flag(true)));
        assert!(!p.on_changed(PER_WINDOW, &SettingValue::Integer(1)));
        assert!(p.on_changed(PER_WINDOW, &SettingValue::Flag(false)));
        assert_eq!(keyboard(&p), keyboard(&InputSettings::default()));
    }
}
