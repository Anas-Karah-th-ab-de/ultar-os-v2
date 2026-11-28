//! The language and the theme of the compositor's own drawing (title bars and their
//! buttons, system dialog frames, the ground): `region.language` and `appearance.theme`
//! become `Request::ConfigureLook`, so a change in Settings shows at once on every window –
//! the buttons of a title bar move to the side the new language reads from.

use ultar_bus::messages::settings::SettingValue;
use ultar_surface::proto::{LookLang, Request};

/// Setting: the system language (`ar`, `en`, `de`).
pub const LANGUAGE: &str = "region.language";
/// Setting: the theme (`dark`, `light`, `auto`).
pub const THEME: &str = "appearance.theme";
/// The settings panels that hold them (read when the compositor comes).
pub const PANELS: [&str; 2] = ["region", "appearance"];

/// What the compositor was last told about its look (or should be).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LookSettings {
    /// The language.
    pub lang: LookLang,
    /// The light theme.
    pub light: bool,
    /// A value came from the settings service (before that nothing is sent: the
    /// compositor keeps what it was started with).
    pub known: bool,
}

impl LookSettings {
    /// Takes a setting that changed; `true` when it was one of these and differs from what
    /// is known. An unknown language changes nothing; `auto` is the dark standard.
    pub fn on_changed(&mut self, key: &str, value: &SettingValue) -> bool {
        let before = *self;
        match (key, value) {
            (LANGUAGE, SettingValue::Text(t)) => {
                if let Some(lang) = LookLang::from_word(t) {
                    self.lang = lang;
                    self.known = true;
                }
            }
            (THEME, SettingValue::Text(t)) => {
                self.light = t == "light";
                self.known = true;
            }
            _ => {}
        }
        *self != before
    }

    /// The request that puts these values into effect (none until a value came).
    pub fn requests(&self) -> Vec<Request> {
        if !self.known {
            return Vec::new();
        }
        vec![Request::ConfigureLook {
            lang: self.lang,
            light: self.light,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_and_theme_reach_the_compositor() {
        let mut p = LookSettings::default();
        assert!(p.requests().is_empty(), "nothing before the service said");
        assert!(!p.on_changed("pointer.speed", &SettingValue::Integer(5)));
        assert!(p.on_changed(LANGUAGE, &SettingValue::Text("de".into())));
        assert!(p.on_changed(THEME, &SettingValue::Text("light".into())));
        assert_eq!(
            p.requests(),
            vec![Request::ConfigureLook {
                lang: LookLang::De,
                light: true
            }]
        );
        assert!(!p.on_changed(LANGUAGE, &SettingValue::Text("xx".into())));
        assert!(!p.on_changed(LANGUAGE, &SettingValue::Flag(true)));
        assert!(p.on_changed(THEME, &SettingValue::Text("auto".into())));
        assert!(!p.light);
    }
}
