//! The catalogs of the phone shell (`locales/{ar,en,de}.ftl`, Arabic first and the reference)
//! on top of the system's base catalogs (weekdays, Hijri months).

use ultar_i18n::{Lang, ParseError, Translator};

/// The catalog source of each language, Arabic first.
pub const SOURCES: [(Lang, &str); 3] = [
    (Lang::Ar, include_str!("../locales/ar.ftl")),
    (Lang::En, include_str!("../locales/en.ftl")),
    (Lang::De, include_str!("../locales/de.ftl")),
];

/// A translator with the base catalogs and the phone shell's.
pub fn translator(lang: Lang) -> Result<Translator, ParseError> {
    let mut tr = Translator::new(lang);
    for (each, source) in SOURCES {
        tr.add_source(each, source)?;
    }
    // The confirmation of apps from other systems is asked by the system dialog; its texts
    // come with the contract (like on the desktop).
    for (tag, source) in ultar_bus::messages::foreign::LOCALES {
        if let Some(each) = Lang::parse(tag) {
            tr.add_source(each, source)?;
        }
    }
    Ok(tr)
}

/// Like [`translator`]; a catalog that cannot be read leaves the keys visible instead of
/// stopping the shell (the catalogs are checked by the tests).
pub fn translator_or_keys(lang: Lang) -> Translator {
    translator(lang).unwrap_or_else(|_| Translator::new(lang))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(source: &str) -> Vec<&str> {
        let mut keys: Vec<&str> = source
            .lines()
            .filter(|l| !l.starts_with('#') && l.contains(" = "))
            .filter_map(|l| l.split(" = ").next())
            .collect();
        keys.sort_unstable();
        keys
    }

    #[test]
    fn the_confirmation_of_apps_from_other_systems_is_in_every_language() {
        for lang in [Lang::Ar, Lang::En, Lang::De] {
            let tr = translator(lang).unwrap();
            let text = tr.t("foreign.confirm.install");
            assert_ne!(text, "foreign.confirm.install", "{lang:?}");
        }
    }

    #[test]
    fn catalogs_parse_and_have_the_same_keys() {
        for (lang, _) in SOURCES {
            translator(lang).unwrap();
        }
        let ar = keys(SOURCES[0].1);
        assert!(!ar.is_empty());
        for (lang, source) in &SOURCES[1..] {
            assert_eq!(keys(source), ar, "{lang:?} has other keys than Arabic");
        }
    }
}
