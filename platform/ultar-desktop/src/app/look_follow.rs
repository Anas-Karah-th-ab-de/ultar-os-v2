//! The look of the settings for the desktop's small programs (system dialogs, on-screen
//! keyboard): language, theme, accent colour and text size are read at start and followed
//! on `settings.changed`, so a dialog speaks the language the person chose – also right
//! after the installation, without a new start.

use crate::view::Look;
use std::time::Duration;
use ultar_bus::messages::settings::{Get, SettingValue};
use ultar_bus::Client;
use ultar_ui::tokens::{Accent, ThemeKind};

/// The settings that change the look.
pub const KEYS: [&str; 4] = [
    "region.language",
    "appearance.theme",
    "appearance.accent_color",
    "appearance.font_size",
];

/// How long one read at start may take.
const WAIT: Duration = Duration::from_millis(500);

/// Puts one setting into `look`; `true` when it was one of [`KEYS`] and changed the look.
pub fn apply(look: &mut Look, key: &str, value: &SettingValue) -> bool {
    let before = *look;
    match (key, value) {
        ("region.language", SettingValue::Text(t)) => {
            if let Some(lang) = ultar_i18n::Lang::parse(t) {
                look.lang = lang;
            }
        }
        ("appearance.theme", SettingValue::Text(t)) => {
            look.theme = if t == "light" {
                ThemeKind::Light
            } else {
                ThemeKind::Dark
            };
        }
        ("appearance.accent_color", SettingValue::Text(t)) => {
            look.accent = Accent::parse(t).unwrap_or_default();
        }
        ("appearance.font_size", SettingValue::Integer(v)) => {
            let (lo, hi) = crate::model::prefs::FONT_RANGE;
            look.text_scale = (*v).clamp(i64::from(lo), i64::from(hi)) as f32 / 100.0;
        }
        _ => return false,
    }
    *look != before
}

/// Reads [`KEYS`] once (best effort: without an answer the look stays).
pub fn read(client: &Client, look: &mut Look) {
    for key in KEYS {
        let reply = client.call(&Get { key: key.into() }, WAIT);
        match reply.ok().and_then(|r| r.value) {
            Some(value) => {
                apply(look, key, &value);
            }
            None => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_i18n::Lang;

    #[test]
    fn the_four_settings_reach_the_look() {
        let mut look = Look::new(Lang::Ar, ThemeKind::Dark);
        let text = |t: &str| SettingValue::Text(t.into());
        assert!(apply(&mut look, "region.language", &text("en")));
        assert_eq!(look.lang, Lang::En);
        assert!(apply(&mut look, "appearance.theme", &text("light")));
        assert_eq!(look.theme, ThemeKind::Light);
        assert!(apply(&mut look, "appearance.accent_color", &text("green")));
        assert_eq!(look.accent, Accent::Green);
        assert!(apply(
            &mut look,
            "appearance.font_size",
            &SettingValue::Integer(120)
        ));
        assert!((look.text_scale - 1.2).abs() < 0.001);
        assert!(
            !apply(&mut look, "region.language", &text("en")),
            "no change"
        );
        assert!(!apply(&mut look, "bar.autohide", &SettingValue::Flag(true)));
    }
}
