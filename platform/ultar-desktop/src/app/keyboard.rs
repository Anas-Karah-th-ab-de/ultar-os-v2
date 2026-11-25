//! The input languages (keyboard layouts) as the settings service keeps them: the list the
//! user switches through – the Keyboard page's `keyboard.input_languages` when it names any,
//! else `region.keyboard.layouts` (e.g. `ar_en`) – and the active one
//! (`region.keyboard.active`). The shell (and the lock screen, which starts first after a
//! restart) gives both to the compositor at start and on every change, and stores the active
//! one when the user switched (the chosen shortcut, the bar's language menu or the sign-in's
//! chip), so it survives a restart.

use crate::ports::SettingsPort;
use ultar_bus::messages::settings::SettingValue;

/// The layouts to switch through, `_`-separated, Arabic first (settings schema).
pub const LAYOUTS: &str = "region.keyboard.layouts";
/// The active layout (`ar`, `en`, `de`).
pub const ACTIVE: &str = "region.keyboard.active";
/// The input languages of the Keyboard page, in order (empty: the region's layouts).
pub const INPUT_LANGUAGES: &str = "keyboard.input_languages";

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

/// The list to switch through: the Keyboard page's input languages when it names any
/// (only known names, no repeats), else the region's layouts.
pub fn combined(input_languages: &[String], region_layouts: &str) -> Vec<String> {
    let langs = names(&input_languages.join("_"));
    if langs.is_empty() {
        names(region_layouts)
    } else {
        langs
    }
}

/// The list and the index of the active layout in it, as the settings say
/// (`None`: no answer or nothing usable).
pub fn wanted(settings: &dyn SettingsPort) -> Option<(Vec<String>, u8)> {
    let text = |key| match settings.get(key) {
        Ok(SettingValue::Text(v)) => Some(v),
        _ => None,
    };
    let langs = match settings.get(INPUT_LANGUAGES) {
        Ok(SettingValue::List(l)) => l,
        _ => Vec::new(),
    };
    from_values(combined(&langs, &text(LAYOUTS)?).join("_"), text(ACTIVE))
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
    use crate::ports::PortError;
    use ultar_bus::messages::settings::DisplayDescriptor;

    /// A settings service that holds a few values.
    struct Fake(Vec<(&'static str, SettingValue)>);
    impl SettingsPort for Fake {
        fn get(&self, key: &str) -> Result<SettingValue, PortError> {
            self.0
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.clone())
                .ok_or(PortError::Unavailable)
        }
        fn set(&self, _: &str, _: SettingValue) -> Result<(), PortError> {
            Err(PortError::Refused)
        }
        fn confirm(&self, _: &str) -> Result<(), PortError> {
            Err(PortError::Refused)
        }
        fn revert(&self, _: &str) -> Result<(), PortError> {
            Err(PortError::Refused)
        }
        fn set_displays(&self, _: &[DisplayDescriptor]) -> Result<Vec<String>, PortError> {
            Err(PortError::Refused)
        }
    }

    fn langs(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_input_languages_win_over_the_regions_layouts() {
        assert_eq!(combined(&langs(&["de", "ar"]), "ar_en"), langs(&["de", "ar"]));
        // Unknown names and repeats are left out; an empty list leaves the region's.
        assert_eq!(combined(&langs(&["xx", "ar", "ar"]), "ar_en"), langs(&["ar"]));
        assert_eq!(combined(&[], "ar_de_en"), langs(&["ar", "de", "en"]));
        assert_eq!(combined(&langs(&["xx"]), "ar_en"), langs(&["ar", "en"]));
        assert!(combined(&[], "").is_empty());
        // As the settings service holds them: the list wins, the active one is found in it.
        let s = Fake(vec![
            (INPUT_LANGUAGES, SettingValue::List(langs(&["de", "en"]))),
            (LAYOUTS, SettingValue::Text("ar_en".into())),
            (ACTIVE, SettingValue::Text("en".into())),
        ]);
        assert_eq!(wanted(&s), Some((langs(&["de", "en"]), 1)));
        // Empty list: the region's layouts, as before.
        let s = Fake(vec![
            (INPUT_LANGUAGES, SettingValue::List(Vec::new())),
            (LAYOUTS, SettingValue::Text("ar_en".into())),
            (ACTIVE, SettingValue::Text("ar".into())),
        ]);
        assert_eq!(wanted(&s), Some((langs(&["ar", "en"]), 0)));
        // A wrong type of the list changes nothing.
        let s = Fake(vec![
            (INPUT_LANGUAGES, SettingValue::Text("de_en".into())),
            (LAYOUTS, SettingValue::Text("ar_en".into())),
        ]);
        assert_eq!(wanted(&s), Some((langs(&["ar", "en"]), 0)));
    }

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
