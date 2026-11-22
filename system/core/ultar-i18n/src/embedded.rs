//! The base catalogs that ship inside the crate (`locales/ar.ftl`, `en.ftl`, `de.ftl`).

use crate::catalog::Catalog;
use crate::error::ParseError;
use crate::lang::Lang;

const AR: &str = include_str!("../locales/ar.ftl");
const EN: &str = include_str!("../locales/en.ftl");
const DE: &str = include_str!("../locales/de.ftl");

/// Source text of the embedded base catalog of a language.
pub const fn source(lang: Lang) -> &'static str {
    match lang {
        Lang::Ar => AR,
        Lang::En => EN,
        Lang::De => DE,
    }
}

/// Parses the embedded base catalog of a language.
///
/// The embedded sources are verified by the crate's tests, so an error here means a broken build.
pub fn catalog(lang: Lang) -> Result<Catalog, ParseError> {
    Catalog::parse(source(lang))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalogs_parse_and_are_seeded() {
        for lang in Lang::ALL {
            let parsed = catalog(lang).unwrap();
            assert!(parsed.len() >= 150, "{lang}: {} keys", parsed.len());
            assert!(source(lang).starts_with("# Ultar OS"));
        }
    }
}
