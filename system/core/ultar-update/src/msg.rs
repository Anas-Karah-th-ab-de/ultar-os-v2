//! The message catalogues of this crate (`locales/{ar,en,de}.ftl`, Arabic first): every state,
//! every error, the history entries, and the texts of the three programs.

use ultar_i18n::{Lang, Translator};

/// Arabic catalogue (the reference).
pub const AR: &str = include_str!("../locales/ar.ftl");
/// English catalogue.
pub const EN: &str = include_str!("../locales/en.ftl");
/// German catalogue.
pub const DE: &str = include_str!("../locales/de.ftl");

/// The catalogue source of each language, Arabic first.
pub const fn sources() -> [(Lang, &'static str); 3] {
    [(Lang::Ar, AR), (Lang::En, EN), (Lang::De, DE)]
}

/// A translator holding this crate's catalogues. A catalogue that fails to parse is left out
/// (the keys themselves are then shown); the tests prove that all three parse.
pub fn translator(lang: Lang) -> Translator {
    let mut translator = Translator::empty(lang);
    for (each, source) in sources() {
        let _ = translator.add_source(each, source);
    }
    translator
}

/// The language named by a `--lang xx` pair in `words`, if any.
pub fn lang_hint(words: &[String]) -> Option<Lang> {
    let at = words.iter().position(|w| w == "--lang")?;
    Lang::parse(words.get(at + 1)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Phase;
    use crate::error::UpdateError;
    use crate::history::HistoryKind;

    #[test]
    fn all_catalogues_parse_and_are_complete() {
        let named: Vec<(String, String)> = [("ar.ftl", AR), ("en.ftl", EN), ("de.ftl", DE)]
            .iter()
            .map(|(n, s)| (n.to_string(), s.to_string()))
            .collect();
        let report = ultar_i18n::check_dir::check_sources(&named).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
    }

    #[test]
    fn every_state_error_and_history_key_has_a_text_in_every_language() {
        let mut keys: Vec<&str> = UpdateError::KEYS.to_vec();
        keys.extend(Phase::ALL.iter().map(|p| p.key()));
        keys.extend(
            [
                HistoryKind::Staged,
                HistoryKind::Applied,
                HistoryKind::Installed,
                HistoryKind::RolledBack,
                HistoryKind::Failed,
                HistoryKind::PreviousSelected,
                HistoryKind::Cancelled,
                HistoryKind::KeysRotated,
            ]
            .iter()
            .map(|k| k.key()),
        );
        for lang in Lang::ALL {
            let tr = translator(lang);
            for key in &keys {
                let text = tr.catalog(lang).get(key);
                assert!(text.is_some(), "{key} missing in {lang:?}");
            }
        }
    }

    #[test]
    fn arabic_is_first_and_the_fallback() {
        assert_eq!(sources()[0].0, Lang::Ar);
        assert_eq!(translator(Lang::Ar).t("update.state.idle"), "النظام محدَّث");
        assert_eq!(
            translator(Lang::En).t("update.error.busy"),
            "An update operation is already running"
        );
        assert_eq!(translator(Lang::De).t("update.cli.yes"), "ja");
        assert_eq!(translator(Lang::En).t("no.such.key"), "no.such.key");
    }

    #[test]
    fn language_hint() {
        let words: Vec<String> = ["status", "--lang", "de"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(lang_hint(&words), Some(Lang::De));
        assert_eq!(lang_hint(&words[..2]), None);
        assert_eq!(lang_hint(&[]), None);
    }
}
