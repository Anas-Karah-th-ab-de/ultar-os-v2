//! The Store's catalogs (APP-RULES §2: every text over `tr`, Arabic first).
//!
//! Keys are in the `store.*` namespace (plus `app.store.*` for the manifest
//! name and description). The `ultar-ui` toolkit adds its own `ui.*` keys on
//! top when a `Ui` is built with one of these translators.

use ultar_i18n::{Lang, ParseError, Translator};

/// Arabic: the reference text.
pub const AR: &str = include_str!("../locales/ar.ftl");
/// English.
pub const EN: &str = include_str!("../locales/en.ftl");
/// German.
pub const DE: &str = include_str!("../locales/de.ftl");

/// The sources with their language.
pub const SOURCES: [(Lang, &str); 3] = [(Lang::Ar, AR), (Lang::En, EN), (Lang::De, DE)];

/// A translator carrying only the Store's texts.
pub fn translator(lang: Lang) -> Result<Translator, ParseError> {
    let mut translator = Translator::empty(lang);
    for (each, source) in SOURCES {
        translator.add_source(each, source)?;
    }
    Ok(translator)
}

/// A translator that also carries the base catalog (`system.*`, `unit.*`) of
/// `ultar-i18n` itself.
pub fn full_translator(lang: Lang) -> Result<Translator, ParseError> {
    let mut translator = Translator::new(lang);
    for (each, source) in SOURCES {
        translator.add_source(each, source)?;
    }
    Ok(translator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_i18n::check;

    /// Every English key exists in Arabic with the same placeholders (and back).
    fn keys(text: &str) -> Vec<String> {
        text.lines()
            .filter_map(|l| l.split_once('=').map(|(k, _)| k.trim().to_string()))
            .filter(|k| !k.starts_with('#'))
            .collect()
    }

    fn placeholders(text: &str, key: &str) -> Vec<String> {
        let mut out = Vec::new();
        let prefix = format!("{key} =");
        let value = text
            .lines()
            .find(|l| l.starts_with(&prefix))
            .unwrap_or_default();
        let mut from = 0;
        while let Some(rel) = value[from..].find('{') {
            let start = from + rel;
            let Some(rel_end) = value[start..].find('}') else {
                break;
            };
            out.push(value[start + 1..start + rel_end].to_string());
            from = start + rel_end + 1;
        }
        out.sort();
        out
    }

    #[test]
    fn catalogs_have_the_same_keys() {
        let ar = keys(AR);
        let en = keys(EN);
        let de = keys(DE);
        for k in &en {
            assert!(ar.contains(k), "{k} has no Arabic text");
            assert!(de.contains(k), "{k} has no German text");
        }
        for k in &ar {
            assert!(en.contains(k), "{k} has no English text");
            assert!(de.contains(k), "{k} has no German text");
        }
    }

    #[test]
    fn placeholders_agree_between_languages() {
        for line in EN.lines() {
            let Some((key, _)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            if key.is_empty() || key.starts_with('#') {
                continue;
            }
            assert_eq!(
                placeholders(EN, key),
                placeholders(AR, key),
                "placeholders differ for {key}"
            );
            assert_eq!(
                placeholders(EN, key),
                placeholders(DE, key),
                "placeholders differ for {key}"
            );
        }
    }

    #[test]
    fn arabic_texts_are_arabic() {
        // Mirrors tools/design-conformance/static_checks.py: an Arabic value
        // may carry Latin brand words, but a value with NO Arabic at all that
        // also spells out a non-allowlisted Latin word is a finding.
        let latin_ok: std::collections::BTreeSet<&str> = [
            "Ultar", "Store", "Firefox", "Linux", "Windows", "Android", "PDF", "USB", "VPN",
            "Mesh", "KM", "Code", "Craft",
        ]
        .into_iter()
        .collect();
        let tr = translator(Lang::Ar).expect("arabic parses");
        for line in AR.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            if key.is_empty() || key.starts_with('#') || value.is_empty() {
                continue;
            }
            let body = value.replace("{server}", "").replace("{ext}", "");
            let has_arabic = body.chars().any(|c| ('؀'..='ۿ').contains(&c));
            let latin_words: Vec<String> = body
                .split(|c: char| !c.is_alphabetic())
                .filter(|w| w.chars().count() >= 4)
                .map(|w| (*w).to_string())
                .filter(|w| !w.chars().all(|c| c.is_uppercase()))
                .filter(|w| !latin_ok.contains(w.as_str()))
                .collect();
            assert!(
                has_arabic || latin_words.is_empty(),
                "{key} is not Arabic: {value}"
            );
            assert!(tr.t(key) != key, "{key} does not resolve");
        }
    }

    #[test]
    fn translator_round_trip() {
        for (lang, source) in SOURCES {
            let mut tr = Translator::empty(lang);
            assert!(
                tr.add_source(lang, source).is_ok(),
                "{lang:?} does not parse"
            );
        }
        let tr = full_translator(Lang::Ar).expect("full translator");
        assert_eq!(tr.t("store.search.placeholder"), "ابحث عن تطبيق أو نوع ملف");
        let ar = ultar_i18n::Catalog::parse(AR).expect("ar catalog");
        let en = ultar_i18n::Catalog::parse(EN).expect("en catalog");
        assert!(check::missing_keys(&en, &ar).is_empty());
    }
}
