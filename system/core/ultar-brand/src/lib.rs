//! # ultar-brand – the one source of the product and company identity
//!
//! **Area:** system core (plan §3.0 area 3). `#![no_std]`, no dependencies, no allocation, so
//! the UEFI loader, the kernel, the services, the toolkit and the apps all read the same data.
//!
//! Everything that names the product or the company comes from here (rules and the list of
//! places: `docs/BRAND.md`; enforced by `ultar-check brand`):
//!
//! | Item | Use |
//! |---|---|
//! | [`PRODUCT`], [`VENDOR`], [`WEBSITE`], [`CONTACT_EMAIL`], [`DEVELOPER`], [`DEVELOPER_AR`] | the facts; identical in every language |
//! | [`product!`], [`vendor!`], [`website!`], [`contact_email!`] … | the same facts as literals, for `concat!` in `const` text |
//! | [`mark`] | the product mark (two open arcs): path data, box, sizes, clear space |
//! | [`logo`] | the company logo: proportions, sizes, clear space, the bitmaps (feature `bitmaps`) |
//! | [`text`] | brand texts in Arabic, English and German (`brand.*` keys, Arabic first) |
//! | [`release`] | the `os-release` file, the user agent of network clients, the console banner |
//!
//! Programs with a translator get every `brand.*` key through `ultar-i18n` (its embedded
//! catalogs include this crate's texts); programs without one (loader, kernel) call
//! [`text::lookup`].

#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod base_apps;
pub mod logo;
pub mod mark;
pub mod release;
pub mod text;

/// The product name as a literal (for `concat!`).
#[macro_export]
macro_rules! product {
    () => {
        "Ultar OS"
    };
}

/// The product name in capitals as a literal: only for the text-mode word mark of the kernel
/// console, the boot picture and the installer header (bitmap font, no logo available).
#[macro_export]
macro_rules! product_caps {
    () => {
        "ULTAR OS"
    };
}

/// The product's machine identifier as a literal (for `concat!`).
#[macro_export]
macro_rules! product_id {
    () => {
        "ultar"
    };
}

/// The company name as a literal (for `concat!`).
#[macro_export]
macro_rules! vendor {
    () => {
        "KM Code Craft"
    };
}

/// The host name of the company website as a literal (for `concat!`).
#[macro_export]
macro_rules! website_host {
    () => {
        "code-craft.tech"
    };
}

/// The company website as a literal (for `concat!`).
#[macro_export]
macro_rules! website {
    () => {
        concat!("https://", $crate::website_host!())
    };
}

/// The public contact address as a literal (for `concat!`).
#[macro_export]
macro_rules! contact_email {
    () => {
        concat!("info@", $crate::website_host!())
    };
}

/// The developer's name in Latin script as a literal (for `concat!`).
#[macro_export]
macro_rules! developer {
    () => {
        "Mohamad Anas Kara Mohamad"
    };
}

/// The developer's name in Arabic script as a literal (for `concat!`).
#[macro_export]
macro_rules! developer_ar {
    () => {
        "محمد أنس قرة محمد"
    };
}

/// The product name. Never translated, never transliterated.
pub const PRODUCT: &str = product!();
/// The product name in capitals (text-mode word mark only, see [`product_caps!`]).
pub const PRODUCT_CAPS: &str = product_caps!();
/// The product's machine identifier (`ID=` of `os-release`, prefixes of file names).
pub const PRODUCT_ID: &str = product_id!();
/// The company / brand name. Never translated, never transliterated.
pub const VENDOR: &str = vendor!();
/// Host name of the company website.
pub const WEBSITE_HOST: &str = website_host!();
/// The company website.
pub const WEBSITE: &str = website!();
/// The public contact address.
pub const CONTACT_EMAIL: &str = contact_email!();
/// The public contact address as a link.
pub const CONTACT_URL: &str = concat!("mailto:", contact_email!());
/// The developer's name in Latin script.
pub const DEVELOPER: &str = developer!();
/// The developer's name in Arabic script.
pub const DEVELOPER_AR: &str = developer_ar!();
/// The `authors` entry of every crate manifest of the repository.
pub const MANIFEST_AUTHORS: &str = concat!(vendor!(), " <", website!(), ">");

/// A fact that is the same in every language, with the key programs read it by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fact {
    /// Message key (`brand.*`).
    pub key: &'static str,
    /// The value.
    pub value: &'static str,
}

/// The facts that are identical in every language. `ultar-i18n` serves them under these keys,
/// so they are never written into a translation file.
pub const FACTS: [Fact; 4] = [
    Fact {
        key: "brand.product",
        value: PRODUCT,
    },
    Fact {
        key: "brand.vendor",
        value: VENDOR,
    },
    Fact {
        key: "brand.website",
        value: WEBSITE,
    },
    Fact {
        key: "brand.contact",
        value: CONTACT_EMAIL,
    },
];

/// The value of a language-independent fact, by key.
pub fn fact(key: &str) -> Option<&'static str> {
    FACTS.iter().find(|f| f.key == key).map(|f| f.value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_are_the_given_ones() {
        assert_eq!(PRODUCT, "Ultar OS");
        assert_eq!(VENDOR, "KM Code Craft");
        assert_eq!(WEBSITE, "https://code-craft.tech");
        assert_eq!(CONTACT_EMAIL, "info@code-craft.tech");
        assert_eq!(CONTACT_URL, "mailto:info@code-craft.tech");
        assert_eq!(DEVELOPER, "Mohamad Anas Kara Mohamad");
        assert_eq!(DEVELOPER_AR, "محمد أنس قرة محمد");
        assert_eq!(MANIFEST_AUTHORS, "KM Code Craft <https://code-craft.tech>");
    }

    #[test]
    fn capitals_and_identifier_are_the_same_name() {
        assert!(PRODUCT.eq_ignore_ascii_case(PRODUCT_CAPS));
        let first_word = PRODUCT.split(' ').next().unwrap_or("");
        assert!(first_word.eq_ignore_ascii_case(PRODUCT_ID));
        assert!(CONTACT_EMAIL.ends_with(WEBSITE_HOST) && WEBSITE.ends_with(WEBSITE_HOST));
    }

    #[test]
    fn facts_are_found_by_key_only() {
        assert_eq!(fact("brand.vendor"), Some(VENDOR));
        assert_eq!(fact("brand.product"), Some(PRODUCT));
        assert_eq!(fact("brand.website"), Some(WEBSITE));
        assert_eq!(fact("brand.contact"), Some(CONTACT_EMAIL));
        assert_eq!(fact("brand.unknown"), None);
        assert_eq!(fact(""), None);
    }

    #[test]
    fn fact_keys_are_unique_and_values_single_line() {
        for (i, a) in FACTS.iter().enumerate() {
            assert!(a.key.starts_with("brand."));
            assert!(!a.value.is_empty() && !a.value.contains('\n'));
            assert!(FACTS.iter().skip(i + 1).all(|b| b.key != a.key));
        }
    }
}
