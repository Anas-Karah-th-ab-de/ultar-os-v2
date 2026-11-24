//! The app's texts (`locales/*.ftl`, Arabic first) in one translator with the toolkit's.

use ultar_i18n::{Lang, Translator};

const AR: &str = include_str!("../locales/ar.ftl");
const EN: &str = include_str!("../locales/en.ftl");
const DE: &str = include_str!("../locales/de.ftl");

/// The catalog source of the app for `lang`.
pub fn source(lang: Lang) -> &'static str {
    match lang {
        Lang::Ar => AR,
        Lang::En => EN,
        Lang::De => DE,
    }
}

/// A translator in `lang` with the app's catalogs of every language.
pub fn translator(lang: Lang) -> Translator {
    let mut tr = Translator::new(lang);
    for l in [Lang::Ar, Lang::En, Lang::De] {
        // Compiled in and tested (tests/texts.rs); a parse error would leave keys visible.
        let _ = tr.add_source(l, source(l));
    }
    tr
}
