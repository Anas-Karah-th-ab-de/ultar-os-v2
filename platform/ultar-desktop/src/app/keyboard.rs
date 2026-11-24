//! The input languages (keyboard layouts) as the settings service keeps them: the list the
//! user switches through (`region.keyboard.layouts`, e.g. `ar_en`) and the active one
//! (`region.keyboard.active`). The shell (and the lock screen, which starts first after a
//! restart) gives both to the compositor at start and on every change, and stores the active
//! one when the user switched (Ctrl+Shift, Alt+Shift, Super+Space, the bar's language menu or
//! the sign-in's chip), so it survives a restart.

use crate::ports::SettingsPort;
use ultar_bus::messages::settings::SettingValue;

/// The layouts to switch through, `_`-separated, Arabic first (settings schema).
pub const LAYOUTS: &str = "region.keyboard.layouts";
/// The active layout (`ar`, `en`, `de`).
pub const ACTIVE: &str = "region.keyboard.active";

/// Layout names of a `region.keyboard.layouts` value (only known names, no repeats).
pub fn names(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in value.split('_') {
        if matches!(n, "ar" | "en" | "de") && !out.iter().any(|o| o == n) {
            out.push(n.to_owned());
        }
    }
    out
}

/// The list and the index of the active layout in it, as the settings say
/// (`None`: no answer or nothing usable).
pub fn wanted(settings: &dyn SettingsPort) -> Option<(Vec<String>, u8)> {
    let text = |key| match settings.get(key) {
        Ok(SettingValue::Text(v)) => Some(v),
        _ => None,
    };
    from_values(text(LAYOUTS)?, text(ACTIVE))
}

/// The list and the active index of the two values (`None`: no usable list).
pub fn from_values(layouts: String, active: Option<String>) -> Option<(Vec<String>, u8)> {
    let list = names(&layouts);
    if list.is_empty() {
        return None;
    }
    let active = active
        .and_then(|a| list.iter().position(|n| *n == a))
        .unwrap_or(0);
    Some((list, u8::try_from(active).unwrap_or(0)))
}

/// What was given to the compositor and what the settings service holds: decides which of
/// the compositor's `KeyboardLayouts` announcements are the user's switches (stored) and
/// which only answer the settings just given (older ones are not stored).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LayoutSync {
    saved: Option<String>,
    pending: Option<(Vec<String>, u8)>,
}

impl LayoutSync {
    /// The settings' list and active index were sent; `known` is the compositor's last
    /// announcement.
    pub fn applied(&mut self, names: &[String], active: u8, known: (&[String], u8)) {
        self.saved = names.get(usize::from(active)).cloned();
        self.pending = (known.0 != names || known.1 != active).then(|| (names.to_vec(), active));
    }

    /// The compositor announced `names`/`active`; returns the name to store (a switch of the
    /// user that the settings do not hold yet).
    pub fn announced(&mut self, names: &[String], active: u8) -> Option<String> {
        if let Some((n, a)) = &self.pending {
            if n == names && *a == active {
                self.pending = None;
            }
            return None;
        }
        let now = names.get(usize::from(active))?;
        (self.saved.as_deref() != Some(now.as_str())).then(|| now.clone())
    }

    /// The settings service took `name`.
    pub fn stored(&mut self, name: String) {
        self.saved = Some(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_lists() {
        assert_eq!(names("ar_en"), vec!["ar", "en"]);
        assert_eq!(names("ar_de_en"), vec!["ar", "de", "en"]);
        assert_eq!(names("ar_xx_ar"), vec!["ar"]);
        assert!(names("").is_empty());
        assert_eq!(
            from_values("ar_de".into(), Some("de".into())),
            Some((vec!["ar".to_owned(), "de".to_owned()], 1))
        );
        assert_eq!(from_values("xx".into(), None), None);
    }

    #[test]
    fn only_the_users_switches_are_stored() {
        let ar_en = vec!["ar".to_owned(), "en".to_owned()];
        let mut s = LayoutSync::default();
        // The settings say English; the compositor still announces its start (Arabic).
        s.applied(&ar_en, 1, (&[], 0));
        assert_eq!(s.announced(&ar_en, 0), None, "older than the settings");
        assert_eq!(s.announced(&ar_en, 1), None, "the answer");
        // Now a switch of the user.
        assert_eq!(s.announced(&ar_en, 0), Some("ar".to_owned()));
        s.stored("ar".into());
        assert_eq!(s.announced(&ar_en, 0), None);
        // Settings equal to what the compositor has: nothing pending.
        s.applied(&ar_en, 0, (&ar_en, 0));
        assert_eq!(s.announced(&ar_en, 1), Some("en".to_owned()));
    }
}
