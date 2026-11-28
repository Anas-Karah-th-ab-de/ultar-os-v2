//! The person's look for every app (`region.language`, `appearance.theme`,
//! `appearance.accent_color`, `appearance.font_size`): the launcher reads it at start and
//! follows `settings.changed` ([`crate::follow`]), and hands it to each app it starts as
//! `ULTAR_LANG`, `ULTAR_THEME`, `ULTAR_ACCENT` and `ULTAR_FONT_SIZE`. An app started after
//! the person changed the language or the theme opens in it – nothing waits for a new
//! start of the system. Only known words and numbers in range pass; anything else keeps the
//! schema's default (the value is the person's, but an app's environment takes no free text).

use ultar_bus::messages::settings::SettingValue;

/// The settings that make the look.
pub const KEYS: [&str; 4] = [
    "region.language",
    "appearance.theme",
    "appearance.accent_color",
    "appearance.font_size",
];

/// Languages of the schema.
const LANGS: [&str; 3] = ["ar", "en", "de"];
/// Themes of the schema.
const THEMES: [&str; 3] = ["dark", "light", "auto"];
/// Accent colours of the schema.
const ACCENTS: [&str; 6] = ["gold", "blue", "green", "red", "purple", "teal"];

/// The look in effect (`None`: the schema's default, nothing is handed over).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Look {
    lang: Option<&'static str>,
    theme: Option<&'static str>,
    accent: Option<&'static str>,
    font_percent: Option<u16>,
}

fn known(list: &'static [&'static str], word: &str) -> Option<&'static str> {
    list.iter().copied().find(|w| *w == word.trim())
}

impl Look {
    /// Takes a changed setting; `true` when it was one of [`KEYS`].
    pub fn on_changed(&mut self, key: &str, value: &SettingValue) -> bool {
        match (key, value) {
            ("region.language", SettingValue::Text(t)) => self.lang = known(&LANGS, t),
            ("appearance.theme", SettingValue::Text(t)) => self.theme = known(&THEMES, t),
            ("appearance.accent_color", SettingValue::Text(t)) => {
                self.accent = known(&ACCENTS, t);
            }
            ("appearance.font_size", SettingValue::Integer(v)) => {
                self.font_percent = u16::try_from((*v).clamp(80, 200)).ok();
            }
            _ => return false,
        }
        true
    }

    /// The environment of an app (only what is known; `auto` hands over nothing).
    pub fn env(&self) -> Vec<(String, String)> {
        let mut env = Vec::new();
        if let Some(lang) = self.lang {
            env.push(("ULTAR_LANG".to_owned(), lang.to_owned()));
        }
        if let Some(theme) = self.theme.filter(|t| *t != "auto") {
            env.push(("ULTAR_THEME".to_owned(), theme.to_owned()));
        }
        if let Some(accent) = self.accent {
            env.push(("ULTAR_ACCENT".to_owned(), accent.to_owned()));
        }
        if let Some(percent) = self.font_percent {
            env.push(("ULTAR_FONT_SIZE".to_owned(), percent.to_string()));
        }
        env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_values_reach_an_app() {
        let mut look = Look::default();
        assert!(look.env().is_empty());
        let text = |t: &str| SettingValue::Text(t.into());
        assert!(look.on_changed("region.language", &text("de")));
        assert!(look.on_changed("appearance.theme", &text("light")));
        assert!(look.on_changed("appearance.accent_color", &text("teal")));
        assert!(look.on_changed("appearance.font_size", &SettingValue::Integer(500)));
        assert!(!look.on_changed("bar.autohide", &SettingValue::Flag(true)));
        let env = look.env();
        assert!(env.contains(&("ULTAR_LANG".into(), "de".into())));
        assert!(env.contains(&("ULTAR_THEME".into(), "light".into())));
        assert!(env.contains(&("ULTAR_ACCENT".into(), "teal".into())));
        assert!(env.contains(&("ULTAR_FONT_SIZE".into(), "200".into())));
        // free text never reaches an environment
        look.on_changed("region.language", &text("de\nLD_PRELOAD=/x"));
        look.on_changed("appearance.theme", &text("auto"));
        let env = look.env();
        assert!(!env
            .iter()
            .any(|(k, _)| k == "ULTAR_LANG" || k == "ULTAR_THEME"));
    }
}
