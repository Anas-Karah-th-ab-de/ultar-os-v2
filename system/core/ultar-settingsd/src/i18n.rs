//! The catalogs of this crate (`locales/{ar,en,de}.ftl`): label and
//! description of every declared setting, labels of all choices, panel
//! titles, and the messages of the daemon and the `usettings` command.

use ultar_bus::messages::settings::StoreNoticeKind;
use ultar_i18n::{Lang, ParseError, Translator};

/// Arabic catalog (the reference).
pub const AR: &str = include_str!("../locales/ar.ftl");
/// English catalog.
pub const EN: &str = include_str!("../locales/en.ftl");
/// German catalog.
pub const DE: &str = include_str!("../locales/de.ftl");

/// The catalog source of each language, Arabic first.
pub const fn sources() -> [(Lang, &'static str); 3] {
    [(Lang::Ar, AR), (Lang::En, EN), (Lang::De, DE)]
}

/// A translator that holds the catalogs of this crate.
pub fn translator(lang: Lang) -> Result<Translator, ParseError> {
    let mut translator = Translator::empty(lang);
    for (each, source) in sources() {
        translator.add_source(each, source)?;
    }
    Ok(translator)
}

/// Key of the message that tells the user about a store notice; its
/// placeholders are `{file}` and `{detail}`.
pub const fn store_notice_key(kind: StoreNoticeKind) -> &'static str {
    match kind {
        StoreNoticeKind::Quarantined => "settingsd.store.quarantined",
        StoreNoticeKind::NewerSchema => "settingsd.store.newer_schema",
        StoreNoticeKind::Migrated => "settingsd.store.migrated",
        StoreNoticeKind::UnknownKeyDropped => "settingsd.store.unknown_key_dropped",
        StoreNoticeKind::InvalidValueDropped => "settingsd.store.invalid_value_dropped",
        StoreNoticeKind::Unreadable => "settingsd.store.unreadable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_i18n::Arg;

    #[test]
    fn catalogs_load_and_arabic_is_the_fallback() {
        let tr = translator(Lang::Ar).unwrap();
        assert_eq!(tr.t("settings.panel.display"), "الشاشات");
        assert_eq!(
            translator(Lang::En).unwrap().t("settings.panel.display"),
            "Displays"
        );
        assert_eq!(
            translator(Lang::De).unwrap().t("settings.panel.display"),
            "Anzeige"
        );
        assert_eq!(tr.t("no.such.key"), "no.such.key");
    }

    #[test]
    fn store_notice_messages_exist_with_their_placeholders() {
        let kinds = [
            StoreNoticeKind::Quarantined,
            StoreNoticeKind::NewerSchema,
            StoreNoticeKind::Migrated,
            StoreNoticeKind::UnknownKeyDropped,
            StoreNoticeKind::InvalidValueDropped,
            StoreNoticeKind::Unreadable,
        ];
        for (lang, _) in sources() {
            let tr = translator(lang).unwrap();
            for kind in kinds {
                let key = store_notice_key(kind);
                assert!(tr.catalog(lang).contains(key), "{key}");
                let args = [
                    ("file", Arg::Str("FILE-NAME")),
                    ("detail", Arg::Str("DETAIL")),
                ];
                let text = tr.t_args(key, &args);
                assert!(text.contains("FILE-NAME"), "{text}");
            }
        }
    }
}
