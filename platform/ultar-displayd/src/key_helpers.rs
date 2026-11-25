//! The keyboard helpers of the accessibility page and the compose key on their way to
//! the compositor: `accessibility.{sticky,slow,bounce,mouse}_keys` and
//! `keyboard.compose_key` become `Request::ConfigureKeyHelpers` (the display service
//! already holds the compositor's configuration right).

use ultar_bus::messages::settings::SettingValue;
use ultar_surface::proto::{ComposeKey, Request};

/// Setting: a modifier pressed and released alone stays active for the next key.
pub const STICKY: &str = "accessibility.sticky_keys";
/// Setting: a key counts only after being held for a moment.
pub const SLOW: &str = "accessibility.slow_keys";
/// Setting: repeats of the same key within a moment are dropped.
pub const BOUNCE: &str = "accessibility.bounce_keys";
/// Setting: the numeric keypad moves the pointer and clicks.
pub const MOUSE: &str = "accessibility.mouse_keys";
/// Setting: the key that starts a compose sequence.
pub const COMPOSE: &str = "keyboard.compose_key";

/// What the compositor was last told about the keyboard helpers (or should be told).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelperSettings {
    /// Sticky keys.
    pub sticky_keys: bool,
    /// Slow keys.
    pub slow_keys: bool,
    /// Bounce keys.
    pub bounce_keys: bool,
    /// Mouse keys.
    pub mouse_keys: bool,
    /// The compose key.
    pub compose_key: ComposeKey,
}

impl Default for HelperSettings {
    /// The defaults of the settings schema (all off, no compose key).
    fn default() -> Self {
        HelperSettings {
            sticky_keys: false,
            slow_keys: false,
            bounce_keys: false,
            mouse_keys: false,
            compose_key: ComposeKey::None,
        }
    }
}

impl HelperSettings {
    /// Takes a setting that changed; `true` when it was one of these and differs from what
    /// is known. Values of the wrong type change nothing; an unknown compose word falls back
    /// to the default ([`ComposeKey::None`]).
    pub fn on_changed(&mut self, key: &str, value: &SettingValue) -> bool {
        let before = *self;
        match (key, value) {
            (STICKY, SettingValue::Flag(on)) => self.sticky_keys = *on,
            (SLOW, SettingValue::Flag(on)) => self.slow_keys = *on,
            (BOUNCE, SettingValue::Flag(on)) => self.bounce_keys = *on,
            (MOUSE, SettingValue::Flag(on)) => self.mouse_keys = *on,
            (COMPOSE, SettingValue::Text(t)) => self.compose_key = ComposeKey::from_word(t),
            _ => {}
        }
        *self != before
    }

    /// The request that puts these values into effect.
    pub fn request(&self) -> Request {
        Request::ConfigureKeyHelpers {
            sticky_keys: self.sticky_keys,
            slow_keys: self.slow_keys,
            bounce_keys: self.bounce_keys,
            mouse_keys: self.mouse_keys,
            compose_key: self.compose_key,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helpers_start_off_and_follow_the_settings() {
        let mut h = HelperSettings::default();
        assert_eq!(h.compose_key, ComposeKey::None);
        assert!(!h.on_changed(STICKY, &SettingValue::Flag(false)));
        assert!(h.on_changed(STICKY, &SettingValue::Flag(true)));
        assert!(h.on_changed(SLOW, &SettingValue::Flag(true)));
        assert!(h.on_changed(BOUNCE, &SettingValue::Flag(true)));
        assert!(h.on_changed(MOUSE, &SettingValue::Flag(true)));
        assert!(h.on_changed(COMPOSE, &SettingValue::Text("menu".into())));
        assert_eq!(
            h.request(),
            Request::ConfigureKeyHelpers {
                sticky_keys: true,
                slow_keys: true,
                bounce_keys: true,
                mouse_keys: true,
                compose_key: ComposeKey::Menu,
            }
        );
        // A runtime change back to the defaults.
        assert!(h.on_changed(STICKY, &SettingValue::Flag(false)));
        assert!(h.on_changed(COMPOSE, &SettingValue::Text("none".into())));
        assert!(!h.sticky_keys);
        assert_eq!(h.compose_key, ComposeKey::None);
    }

    #[test]
    fn wrong_types_change_nothing_and_unknown_words_fall_back() {
        let mut h = HelperSettings::default();
        assert!(!h.on_changed(STICKY, &SettingValue::Integer(1)));
        assert!(!h.on_changed(SLOW, &SettingValue::Text("on".into())));
        assert!(!h.on_changed(COMPOSE, &SettingValue::Flag(true)));
        assert!(!h.on_changed("pointer.speed", &SettingValue::Integer(5)));
        assert!(h.on_changed(COMPOSE, &SettingValue::Text("alt_gr".into())));
        assert_eq!(h.compose_key, ComposeKey::AltGr);
        // An unknown word is the default, not a stored nonsense value.
        assert!(h.on_changed(COMPOSE, &SettingValue::Text("super".into())));
        assert_eq!(h.compose_key, ComposeKey::None);
    }
}
