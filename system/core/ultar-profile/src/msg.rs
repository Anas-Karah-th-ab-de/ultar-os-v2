//! Message table of this crate: Arabic first, then English and German.
//!
//! The logic of the crate only hands out stable keys. This table is the interim home
//! of the texts until the shared i18n crate takes them over; the keys stay the same.

/// Language of a message. Arabic is the reference language of the system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Lang {
    /// Arabic (reference language).
    #[default]
    Ar,
    /// English.
    En,
    /// German.
    De,
}

impl Lang {
    /// Every language of the table, Arabic first.
    pub const ALL: [Lang; 3] = [Lang::Ar, Lang::En, Lang::De];

    /// Parses a language tag such as `ar`, `en_US` or `de-DE`.
    pub fn parse(tag: &str) -> Option<Lang> {
        let lower = tag.trim().to_ascii_lowercase();
        match lower.split(['-', '_', '.']).next() {
            Some("ar") => Some(Lang::Ar),
            Some("en") => Some(Lang::En),
            Some("de") => Some(Lang::De),
            _ => None,
        }
    }

    const fn column(self) -> usize {
        match self {
            Lang::Ar => 0,
            Lang::En => 1,
            Lang::De => 2,
        }
    }
}

/// Key followed by the texts in the order Arabic, English, German.
type Row = (&'static str, [&'static str; 3]);

const TABLE: &[Row] = &[
    (
        "profile.edition.phone",
        ["إصدار الهاتف", "Phone edition", "Handy-Ausgabe"],
    ),
    (
        "profile.edition.desktop",
        [
            "إصدار الحاسوب",
            "Desktop/laptop edition",
            "Desktop-/Laptop-Ausgabe",
        ],
    ),
    (
        "profile.edition.server",
        ["إصدار الخادم", "Server edition", "Server-Ausgabe"],
    ),
    ("profile.form_factor.phone", ["هاتف", "Phone", "Handy"]),
    (
        "profile.form_factor.tablet",
        ["جهاز لوحي", "Tablet", "Tablet"],
    ),
    (
        "profile.form_factor.laptop",
        ["حاسوب محمول", "Laptop", "Laptop"],
    ),
    (
        "profile.form_factor.desktop",
        ["حاسوب مكتبي", "Desktop computer", "Desktop-Rechner"],
    ),
    ("profile.form_factor.server", ["خادم", "Server", "Server"]),
    (
        "profile.form_factor.board",
        ["لوحة إلكترونية", "Board", "Platine"],
    ),
    (
        "profile.mode.mobile",
        ["وضع الهاتف", "Mobile mode", "Mobil-Modus"],
    ),
    (
        "profile.mode.desktop",
        ["وضع سطح المكتب", "Desktop mode", "Desktop-Modus"],
    ),
    (
        "profile.mode.headless",
        ["بدون شاشة", "Headless", "Ohne Oberfläche"],
    ),
    (
        "profile.error.parse",
        [
            "تعذّرت قراءة ملف تعريف الجهاز",
            "The device profile could not be read",
            "Das Geräteprofil konnte nicht gelesen werden",
        ],
    ),
    (
        "profile.error.serialize",
        [
            "تعذّرت كتابة ملف تعريف الجهاز",
            "The device profile could not be written",
            "Das Geräteprofil konnte nicht geschrieben werden",
        ],
    ),
    (
        "profile.error.device_mismatch",
        [
            "ملف التعريف المحفوظ يخصّ جهازًا آخر أو نواة أخرى",
            "The stored profile belongs to another device or kernel",
            "Das gespeicherte Profil gehört zu einem anderen Gerät oder Kernel",
        ],
    ),
];

/// Every key of the table.
pub fn keys() -> impl Iterator<Item = &'static str> {
    TABLE.iter().map(|(key, _)| *key)
}

/// Text of a key in the given language, or `None` for an unknown key.
pub fn text(key: &str, lang: Lang) -> Option<&'static str> {
    TABLE
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .map(|(_, texts)| texts[lang.column()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_every_language() {
        for key in keys() {
            for lang in Lang::ALL {
                assert!(
                    text(key, lang).is_some_and(|value| !value.is_empty()),
                    "{key} {lang:?}"
                );
            }
        }
    }

    #[test]
    fn arabic_texts_are_arabic() {
        for key in keys() {
            let arabic = text(key, Lang::Ar).unwrap();
            assert!(
                arabic
                    .chars()
                    .any(|c| ('\u{0600}'..='\u{06ff}').contains(&c)),
                "{key}"
            );
            assert!(!arabic.chars().any(|c| c.is_ascii_alphabetic()), "{key}");
        }
    }

    #[test]
    fn keys_are_unique() {
        let mut all: Vec<&str> = keys().collect();
        let count = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), count);
    }

    #[test]
    fn unknown_key_and_language_tags() {
        assert_eq!(text("profile.nothing", Lang::En), None);
        assert_eq!(Lang::parse("ar_SY.UTF-8"), Some(Lang::Ar));
        assert_eq!(Lang::parse("EN-us"), Some(Lang::En));
        assert_eq!(Lang::parse("de"), Some(Lang::De));
        assert_eq!(Lang::parse("fr"), None);
        assert_eq!(Lang::parse(""), None);
        assert_eq!(Lang::default(), Lang::Ar);
    }
}
