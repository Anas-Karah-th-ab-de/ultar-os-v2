//! Texts of the command line (`ultar-authd passwd`): keys of the catalogs in
//! `locales/` (Arabic first), loaded into an `ultar-i18n` translator.

use ultar_i18n::{Lang, Translator};

/// The catalogs, Arabic first.
pub const CATALOGS: [(Lang, &str); 3] = [
    (Lang::Ar, include_str!("../locales/ar.ftl")),
    (Lang::En, include_str!("../locales/en.ftl")),
    (Lang::De, include_str!("../locales/de.ftl")),
];

/// A translator for `lang` (keys are printed when a catalog cannot be read).
pub fn translator(lang: Lang) -> Translator {
    let mut tr = Translator::empty(lang);
    for (catalog_lang, source) in CATALOGS {
        let _ = tr.add_source(catalog_lang, source);
    }
    tr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_exists_in_every_language() {
        for lang in [Lang::Ar, Lang::En, Lang::De] {
            let tr = translator(lang);
            for key in [
                "auth.cli.usage",
                "auth.cli.done",
                "auth.cli.mismatch",
                "auth.cli.rejected",
                "auth.cli.failed",
                "auth.daemon.unusable",
            ] {
                let text = tr.t(key);
                assert_ne!(text, key, "{lang:?} {key}");
            }
        }
    }
}
