//! The person's look handed to an app by the app launcher (`ULTAR_THEME`, `ULTAR_ACCENT`,
//! `ULTAR_FONT_SIZE`, see `ultar-launcher`'s `look`): an app started after the person changed
//! the theme, the accent colour or the text size in Settings opens in them.

use crate::paint::PaintEnv;
use crate::tokens::ThemeKind;

/// Environment variable with the session's theme (`dark` | `light`), set by the app launcher.
pub const SESSION_THEME: &str = "ULTAR_THEME";
/// Environment variable with the session's accent colour (`gold`, `blue` …).
pub const SESSION_ACCENT: &str = "ULTAR_ACCENT";
/// Environment variable with the session's text size in percent (80–200).
pub const SESSION_FONT_SIZE: &str = "ULTAR_FONT_SIZE";

impl PaintEnv {
    /// The look the app launcher hands every app (environment `ULTAR_THEME` = `dark` |
    /// `light`, `ULTAR_ACCENT` = an accent word, `ULTAR_FONT_SIZE` = percent 80–200, from the
    /// person's settings): an app started after a change looks like the rest of the system.
    /// `light` (the app's `--light` option) wins over the environment.
    pub fn with_session(self, light: bool) -> PaintEnv {
        let var = |name: &str| std::env::var(name).ok();
        self.with_session_values(
            light,
            var(SESSION_THEME).as_deref(),
            var(SESSION_ACCENT).as_deref(),
            var(SESSION_FONT_SIZE).as_deref(),
        )
    }

    /// [`PaintEnv::with_session`] with the values given (tests, programs with options).
    pub fn with_session_values(
        mut self,
        light: bool,
        theme: Option<&str>,
        accent: Option<&str>,
        font_size: Option<&str>,
    ) -> PaintEnv {
        self.theme = match (light, theme.map(str::trim)) {
            (true, _) | (false, Some("light")) => ThemeKind::Light,
            (false, Some("dark")) => ThemeKind::Dark,
            _ => self.theme,
        };
        if let Some(accent) = accent.and_then(crate::tokens::Accent::parse) {
            self.accent = accent;
        }
        if let Some(percent) = font_size.and_then(|f| f.trim().parse::<u16>().ok()) {
            self.text_scale = f32::from(percent.clamp(80, 200)) / 100.0;
        }
        self
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;
    use crate::tokens::{Accent, ColorRole, THEME};
    use ultar_i18n::Lang;

    #[test]
    fn the_session_look_reaches_an_app() {
        let env = PaintEnv::new(Lang::Ar, ThemeKind::Dark);
        let s = env.with_session_values(false, Some("light"), Some("teal"), Some("150"));
        assert_eq!(s.theme, ThemeKind::Light);
        assert_eq!(s.accent, Accent::Teal);
        assert!((s.text_scale - 1.5).abs() < 0.001);
        assert_ne!(
            s.color(ColorRole::Accent),
            THEME.palette(ThemeKind::Light).get(ColorRole::Accent)
        );
        // nothing set: unchanged; nonsense: ignored; --light wins
        assert_eq!(env.with_session_values(false, None, None, None), env);
        let odd = env.with_session_values(false, Some("pink"), Some("pink"), Some("x"));
        assert_eq!(odd, env);
        let forced = env.with_session_values(true, Some("dark"), None, None);
        assert_eq!(forced.theme, ThemeKind::Light);
        // high contrast keeps its own accent
        let mut hc = s;
        hc.high_contrast = true;
        assert_eq!(
            hc.color(ColorRole::Accent),
            crate::tokens::HIGH_CONTRAST.get(ColorRole::Accent)
        );
    }
}
