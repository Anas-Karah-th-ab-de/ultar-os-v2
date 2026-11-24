//! The browser's texts (`locales/{ar,en,de}.ftl`, Arabic first) on top of the system catalogs.

use ultar_i18n::{Lang, ParseError, Translator};

/// The catalogs of the browser.
pub const SOURCES: [(Lang, &str); 3] = [
    (Lang::Ar, include_str!("../locales/ar.ftl")),
    (Lang::En, include_str!("../locales/en.ftl")),
    (Lang::De, include_str!("../locales/de.ftl")),
];

/// A translator for `lang` with the system and browser catalogs.
pub fn translator(lang: Lang) -> Result<Translator, ParseError> {
    let mut tr = Translator::new(lang);
    for (l, source) in SOURCES {
        tr.add_source(l, source)?;
    }
    Ok(tr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_every_key() {
        let keys = |src: &str| -> Vec<String> {
            let mut k: Vec<String> = src
                .lines()
                .filter(|l| !l.starts_with('#') && l.contains(" = "))
                .filter_map(|l| l.split(" = ").next().map(str::to_string))
                .collect();
            k.sort();
            k
        };
        let ar = keys(SOURCES[0].1);
        assert!(ar.len() > 50);
        for (_, src) in &SOURCES[1..] {
            assert_eq!(keys(src), ar);
        }
        let tr = translator(Lang::Ar).unwrap();
        assert_eq!(tr.t("web.back"), "رجوع");
        assert!(translator(Lang::De).unwrap().has("web.dl.show"));
    }
}
