//! The catalogs of the desktop (`locales/{ar,en,de}.ftl`, Arabic first and the reference)
//! on top of the system's base catalogs (months, weekdays) and the toolkit's own.

use ultar_i18n::{Lang, ParseError, Translator};

/// The catalog source of each language, Arabic first.
pub const SOURCES: [(Lang, &str); 3] = [
    (Lang::Ar, include_str!("../locales/ar.ftl")),
    (Lang::En, include_str!("../locales/en.ftl")),
    (Lang::De, include_str!("../locales/de.ftl")),
];

/// A translator with the base catalogs and the desktop's.
pub fn translator(lang: Lang) -> Result<Translator, ParseError> {
    let mut tr = Translator::new(lang);
    for (each, source) in SOURCES {
        tr.add_source(each, source)?;
    }
    // The helper's answers are shown by the shell, the confirmation of apps from other
    // systems by the system dialog: their texts come with their contracts.
    let contracts = ultar_bus::messages::agent::LOCALES
        .into_iter()
        .chain(ultar_bus::messages::foreign::LOCALES);
    for (tag, source) in contracts {
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

    #[test]
    fn the_update_notification_keys_exist_in_every_language() {
        // The update service posts these keys (ultar-update notify.rs); the shell renders them.
        for lang in Lang::ALL {
            let tr = translator(lang).unwrap();
            for key in [
                "desk.note.app.update",
                "desk.note.update.available",
                "desk.note.update.restart",
                "desk.note.update.usb",
                "desk.note.update.usb.body",
            ] {
                assert!(tr.catalog(lang).get(key).is_some(), "{key} in {lang:?}");
            }
        }
    }

    #[test]
    fn catalogs_parse_and_have_the_same_keys() {
        let keys = |lang: Lang| {
            let tr = translator(lang).unwrap();
            let mut k: Vec<String> = tr.catalog(lang).keys().map(str::to_owned).collect();
            k.sort();
            k
        };
        let ar = keys(Lang::Ar);
        assert!(ar.iter().any(|k| k.starts_with("desk.")));
        let desk = |v: Vec<String>| -> Vec<String> {
            v.into_iter()
                .filter(|k| {
                    ["desk.", "lock.", "dialog."]
                        .iter()
                        .any(|p| k.starts_with(p))
                })
                .collect()
        };
        assert_eq!(desk(ar.clone()), desk(keys(Lang::En)));
        assert_eq!(desk(ar), desk(keys(Lang::De)));
    }
}
