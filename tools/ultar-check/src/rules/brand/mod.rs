//! Rule `brand`: the product and company identity comes from one source (`docs/BRAND.md`).
//!
//! The source is the crate `system/core/ultar-brand` (names, website, contact, developer,
//! brand mark, brand texts in Arabic, English and German). The rule fails when
//!
//! * a string literal outside that crate (tests excepted) spells one of the names, the
//!   website or the contact address instead of using the constants or macros of the crate,
//! * the brand texts do not cover the same keys in every language, a `brand.*` key lives in
//!   another catalog, or a catalog spells a name differently (`catalogs`),
//! * `rootfs/etc/os-release`, the `authors` of a crate manifest or the brand mark of the icon
//!   set differ from the source (`files`).

mod catalogs;
mod files;

use super::{Context, Rule};
use crate::error::CheckError;
use crate::finding::{excerpt, Finding, Message};
use crate::msg::Key;
use crate::scan::{SourceFile, TokenKind};

/// The crate that is the source; its literals are the definitions.
pub const SOURCE_CRATE: &str = "system/core/ultar-brand/";

/// The names whose spelling is fixed, lower case, with the canonical form.
pub(crate) fn names() -> [(&'static str, String); 2] {
    [
        (ultar_brand::PRODUCT, ultar_brand::PRODUCT.to_lowercase()),
        (ultar_brand::VENDOR, ultar_brand::VENDOR.to_lowercase()),
    ]
}

/// Whether `text` names the company's own host (not a service below it such as
/// `store.<host>`): the website or a mail address there.
pub(crate) fn own_host_position(text: &str) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    let host = ultar_brand::WEBSITE_HOST;
    let mut from = 0;
    while let Some(found) = lower.get(from..).and_then(|rest| rest.find(host)) {
        let at = from + found;
        let before = lower.get(..at).and_then(|head| head.chars().next_back());
        if before != Some('.') {
            return Some(at);
        }
        from = at + host.len();
    }
    None
}

/// The brand term a text spells out, if any (the canonical form is returned).
pub fn brand_term(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    if let Some((name, _)) = names().into_iter().find(|(_, low)| lower.contains(low)) {
        return Some(name);
    }
    if lower.contains(&ultar_brand::DEVELOPER.to_lowercase()) {
        return Some(ultar_brand::DEVELOPER);
    }
    if text.contains(ultar_brand::DEVELOPER_AR) {
        return Some(ultar_brand::DEVELOPER_AR);
    }
    let at = own_host_position(text)?;
    let mail = text.get(..at).is_some_and(|head| head.ends_with('@'));
    Some(if mail {
        ultar_brand::CONTACT_EMAIL
    } else {
        ultar_brand::WEBSITE
    })
}

/// Flags string literals outside the source crate that spell a brand term.
pub fn check_literals(files: &[SourceFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files.iter().filter(|f| !f.is_under(&[SOURCE_CRATE])) {
        for token in &file.code {
            if token.kind != TokenKind::Str || token.in_test {
                continue;
            }
            if let Some(term) = brand_term(&token.text) {
                let message =
                    Message::new(Key::BrandLiteral, &[term.to_string(), excerpt(&token.text)]);
                findings.push(Finding::error(Rule::Brand, &file.path, token.line, message));
            }
        }
    }
    findings
}

/// Runs every part of the rule.
pub fn check(context: &Context<'_>) -> Result<Vec<Finding>, CheckError> {
    let mut findings = check_literals(context.files);
    findings.extend(catalogs::check(context.root)?);
    findings.extend(files::check(context.root, context.files)?);
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_are_found_in_any_case() {
        assert_eq!(
            brand_term("Welcome to ULTAR OS"),
            Some(ultar_brand::PRODUCT)
        );
        assert_eq!(brand_term("by km code craft"), Some(ultar_brand::VENDOR));
        assert_eq!(
            brand_term("mohamad anas kara mohamad"),
            Some(ultar_brand::DEVELOPER)
        );
        assert_eq!(
            brand_term(ultar_brand::DEVELOPER_AR),
            Some(ultar_brand::DEVELOPER_AR)
        );
        assert_eq!(brand_term("ultar-os"), None);
        assert_eq!(brand_term("Ultar"), None);
        assert_eq!(brand_term(""), None);
    }

    #[test]
    fn own_host_is_a_term_but_services_below_it_are_not() {
        assert_eq!(
            brand_term("https://Code-Craft.tech/x"),
            Some(ultar_brand::WEBSITE)
        );
        assert_eq!(
            brand_term("someone@code-craft.tech"),
            Some(ultar_brand::CONTACT_EMAIL)
        );
        assert_eq!(brand_term("https://store.code-craft.tech"), None);
        assert_eq!(
            brand_term("a.code-craft.tech and code-craft.tech"),
            Some(ultar_brand::WEBSITE)
        );
        assert_eq!(own_host_position("x.code-craft.tech"), None);
    }
}
