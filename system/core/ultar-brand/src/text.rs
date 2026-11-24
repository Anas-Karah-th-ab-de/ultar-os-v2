//! Brand texts in Arabic, English and German (`locales/*.ftl`, `ultar-i18n` format, one line
//! per message, no placeholders).
//!
//! Programs with a translator read these keys through `ultar-i18n`, which embeds the sources
//! below. The loader and the kernel have no translator and call [`lookup`]. The reference
//! language is Arabic: a key missing in another language falls back to it.

/// A language the brand texts exist in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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
    /// Every language, Arabic first.
    pub const ALL: [Lang; 3] = [Lang::Ar, Lang::En, Lang::De];

    /// The two-letter code.
    pub const fn code(self) -> &'static str {
        match self {
            Lang::Ar => "ar",
            Lang::En => "en",
            Lang::De => "de",
        }
    }

    /// Reads a language from a code or locale name (`ar`, `en_US.UTF-8`, `de-DE`).
    pub fn from_code(code: &str) -> Option<Lang> {
        let head = code.split(['_', '-', '.', '@']).next().unwrap_or("");
        Lang::ALL
            .into_iter()
            .find(|lang| head.eq_ignore_ascii_case(lang.code()))
    }
}

const AR: &str = include_str!("../locales/ar.ftl");
const EN: &str = include_str!("../locales/en.ftl");
const DE: &str = include_str!("../locales/de.ftl");

/// Source text of the brand catalog of a language (`ultar-i18n` catalog format).
pub const fn source(lang: Lang) -> &'static str {
    match lang {
        Lang::Ar => AR,
        Lang::En => EN,
        Lang::De => DE,
    }
}

/// The messages of a catalog source as `(key, text)` pairs, in file order.
pub fn entries(source: &'static str) -> impl Iterator<Item = (&'static str, &'static str)> {
    source.lines().filter_map(|line| {
        if line.starts_with('#') || line.starts_with([' ', '\t']) {
            return None;
        }
        let (key, value) = line.split_once('=')?;
        let (key, value) = (key.trim(), value.trim());
        (!key.is_empty() && !value.is_empty()).then_some((key, value))
    })
}

/// The keys of the brand texts (those of the Arabic reference), in file order.
pub fn keys() -> impl Iterator<Item = &'static str> {
    entries(AR).map(|(key, _)| key)
}

fn find(lang: Lang, key: &str) -> Option<&'static str> {
    entries(source(lang))
        .find(|(k, _)| *k == key)
        .map(|(_, value)| value)
}

/// The text of `key` in `lang`: a language-independent fact ([`crate::FACTS`]) first, then
/// the catalog of the language, then the Arabic one. `None` for an unknown key.
pub fn lookup(lang: Lang, key: &str) -> Option<&'static str> {
    crate::fact(key)
        .or_else(|| find(lang, key))
        .or_else(|| find(Lang::Ar, key))
}

/// The developer's name in the script of the language.
pub const fn developer(lang: Lang) -> &'static str {
    match lang {
        Lang::Ar => crate::DEVELOPER_AR,
        Lang::En | Lang::De => crate::DEVELOPER,
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    fn is_arabic(c: char) -> bool {
        matches!(u32::from(c), 0x0600..=0x06FF)
    }

    #[test]
    fn codes_round_trip_and_locale_names_are_read() {
        for lang in Lang::ALL {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
        assert_eq!(Lang::from_code("ar_SY.UTF-8"), Some(Lang::Ar));
        assert_eq!(Lang::from_code("EN-us"), Some(Lang::En));
        assert_eq!(Lang::from_code("de@euro"), Some(Lang::De));
        assert_eq!(Lang::from_code("fr"), None);
        assert_eq!(Lang::from_code(""), None);
        assert_eq!(Lang::default(), Lang::Ar);
        assert_eq!(Lang::ALL[0], Lang::Ar);
    }

    #[test]
    fn every_language_has_exactly_the_reference_keys() {
        let reference: [&str; 64] = {
            let mut out = [""; 64];
            for (slot, key) in out.iter_mut().zip(keys()) {
                *slot = key;
            }
            out
        };
        let count = keys().count();
        assert!(count >= 20 && count <= reference.len());
        for lang in Lang::ALL {
            let mut seen = 0;
            for (key, value) in entries(source(lang)) {
                assert!(reference[..count].contains(&key), "{lang:?}: extra {key}");
                assert!(key.starts_with("brand."), "{key}");
                assert!(!value.contains('{'), "{key}: placeholders are not allowed");
                seen += 1;
            }
            assert_eq!(seen, count, "{lang:?}");
            for key in &reference[..count] {
                assert!(find(lang, key).is_some(), "{lang:?}: missing {key}");
            }
        }
    }

    #[test]
    fn keys_are_unique_and_do_not_shadow_a_fact() {
        for lang in Lang::ALL {
            for (i, (key, _)) in entries(source(lang)).enumerate() {
                assert!(crate::fact(key).is_none(), "{key} is a fact");
                assert!(
                    entries(source(lang)).skip(i + 1).all(|(k, _)| k != key),
                    "{lang:?}: {key} twice"
                );
            }
        }
    }

    #[test]
    fn arabic_texts_are_arabic_and_names_are_spelled_as_the_facts() {
        for (key, value) in entries(AR) {
            assert!(value.chars().any(is_arabic), "{key}");
        }
        for lang in Lang::ALL {
            for (key, value) in entries(source(lang)) {
                let lower = value.to_ascii_lowercase();
                for name in [crate::PRODUCT, crate::VENDOR] {
                    let hits = lower.matches(name.to_ascii_lowercase().as_str()).count();
                    assert_eq!(hits, value.matches(name).count(), "{key}");
                }
                if value.contains('@') {
                    assert!(value.contains(crate::CONTACT_EMAIL), "{key}");
                }
            }
        }
    }

    #[test]
    fn lookup_prefers_facts_then_language_then_arabic() {
        assert_eq!(lookup(Lang::En, "brand.vendor"), Some(crate::VENDOR));
        assert_eq!(lookup(Lang::Ar, "brand.product"), Some(crate::PRODUCT));
        assert_eq!(
            lookup(Lang::Ar, "brand.developer"),
            Some(crate::DEVELOPER_AR)
        );
        assert_eq!(lookup(Lang::En, "brand.developer"), Some(crate::DEVELOPER));
        assert_eq!(lookup(Lang::De, "brand.developer"), Some(crate::DEVELOPER));
        assert_eq!(lookup(Lang::En, "brand.about.version"), Some("Version"));
        assert_eq!(lookup(Lang::En, "brand.nope"), None);
        assert_eq!(lookup(Lang::En, ""), None);
        assert_eq!(lookup(Lang::En, "#"), None);
    }

    #[test]
    fn developer_follows_the_script() {
        assert_eq!(developer(Lang::Ar), crate::DEVELOPER_AR);
        assert_eq!(developer(Lang::En), crate::DEVELOPER);
        assert_eq!(developer(Lang::De), crate::DEVELOPER);
        for lang in Lang::ALL {
            assert_eq!(find(lang, "brand.developer"), Some(developer(lang)));
            let line = find(lang, "brand.developed_by").unwrap();
            assert!(line.ends_with(developer(lang)), "{lang:?}");
        }
    }

    #[test]
    fn crash_texts_name_the_company_and_the_contact() {
        for lang in Lang::ALL {
            for key in ["brand.crash.kernel", "brand.crash.report.send"] {
                let text = find(lang, key).unwrap();
                assert!(text.contains(crate::VENDOR), "{lang:?} {key}");
                assert!(text.contains(crate::CONTACT_EMAIL), "{lang:?} {key}");
            }
        }
    }

    #[test]
    fn entries_skip_comments_blank_and_broken_lines() {
        let source = "# c\n\nkey = value\n  continuation\nnovalue =\n= nokey\nplain\na=b=c\n";
        let mut it = entries(source);
        assert_eq!(it.next(), Some(("key", "value")));
        assert_eq!(it.next(), Some(("a", "b=c")));
        assert_eq!(it.next(), None);
    }
}
