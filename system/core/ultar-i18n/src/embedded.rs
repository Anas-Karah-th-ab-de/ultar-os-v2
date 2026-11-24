//! The base catalogs that ship inside the crate (`locales/ar.ftl`, `en.ftl`, `de.ftl`), together
//! with the brand texts of `ultar-brand` (`brand.*`): every program with a translator reads the
//! product name, the company and the brand texts from that one source.

use crate::catalog::Catalog;
use crate::error::ParseError;
use crate::lang::Lang;
use alloc::string::String;

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
    let mut catalog = Catalog::parse(source(lang))?;
    catalog.merge(Catalog::parse(brand_source(lang))?);
    catalog.merge(Catalog::parse(&brand_facts())?);
    Ok(catalog)
}

/// Source text of the brand catalog of a language (from `ultar-brand`).
pub const fn brand_source(lang: Lang) -> &'static str {
    ultar_brand::text::source(match lang {
        Lang::Ar => ultar_brand::text::Lang::Ar,
        Lang::En => ultar_brand::text::Lang::En,
        Lang::De => ultar_brand::text::Lang::De,
    })
}

/// The language-independent brand facts (`brand.product`, `brand.vendor`, `brand.website`,
/// `brand.contact`) as catalog source. They are never written into a translation file.
pub fn brand_facts() -> String {
    let mut source = String::new();
    for fact in ultar_brand::FACTS {
        source.push_str(fact.key);
        source.push_str(" = ");
        source.push_str(fact.value);
        source.push('\n');
    }
    source
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

    #[test]
    fn brand_texts_and_facts_are_served_in_every_language() {
        for lang in Lang::ALL {
            let parsed = catalog(lang).unwrap();
            let text = |key: &str| parsed.get(key).and_then(|m| m.as_static_text());
            for fact in ultar_brand::FACTS {
                assert_eq!(text(fact.key), Some(fact.value), "{lang}");
            }
            for key in ultar_brand::text::keys() {
                assert!(parsed.contains(key), "{lang}: {key}");
            }
            // the older names of the same facts must say the same
            assert_eq!(text("system.name"), Some(ultar_brand::PRODUCT));
            assert_eq!(text("system.vendor"), Some(ultar_brand::VENDOR));
        }
        let arabic = catalog(Lang::Ar).unwrap();
        let developer = arabic
            .get("brand.developer")
            .and_then(|m| m.as_static_text());
        assert_eq!(developer, Some(ultar_brand::DEVELOPER_AR));
    }

    #[test]
    fn brand_keys_do_not_collide_with_the_base_catalog() {
        for lang in Lang::ALL {
            let base = Catalog::parse(source(lang)).unwrap();
            assert_eq!(base.keys_with_prefix("brand.").count(), 0, "{lang}");
        }
    }
}
