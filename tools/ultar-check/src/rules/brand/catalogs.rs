//! Translation catalogs (`*.ftl`): coverage of the brand texts and spelling of the names.

use super::{names, own_host_position, SOURCE_CRATE};
use crate::error::CheckError;
use crate::finding::{Finding, Message};
use crate::msg::Key;
use crate::repo;
use crate::rules::text::is_arabic;
use crate::rules::Rule;
use std::path::Path;

/// Directory of the brand texts.
const BRAND_LOCALES: &str = "system/core/ultar-brand/locales";

/// The languages every brand text exists in; the first is the reference.
const LANGS: [&str; 3] = ["ar", "en", "de"];

/// Namespace of the brand texts.
const NAMESPACE: &str = "brand.";

/// Older keys of other catalogs that repeat a fact: `(key, fact key)`.
const ALIASES: [(&str, &str); 2] = [
    ("system.name", "brand.product"),
    ("system.vendor", "brand.vendor"),
];

/// One `key = value` line of a catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) key: String,
    pub(crate) value: String,
    pub(crate) line: u32,
}

/// The messages of a catalog (first line of each; continuation lines are joined).
pub(crate) fn entries(text: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        if line.starts_with([' ', '\t']) {
            if let Some(last) = out.last_mut() {
                last.value.push(' ');
                last.value.push_str(line.trim());
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            if !key.is_empty() && !key.contains(' ') {
                out.push(Entry {
                    key: key.to_string(),
                    value: value.trim().to_string(),
                    line: number,
                });
            }
        }
    }
    out
}

/// A name written with other capitals: the text found.
fn misspelled<'a>(value: &'a str, canonical: &str, lower_name: &str) -> Option<&'a str> {
    let lower = value.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower.get(from..).and_then(|rest| rest.find(lower_name)) {
        let at = from + found;
        let end = at + lower_name.len();
        let written = value.get(at..end)?;
        if written != canonical {
            return Some(written);
        }
        from = end;
    }
    None
}

/// A mail address at the company's own host that is not the public contact address.
fn wrong_contact(value: &str) -> bool {
    let Some(at) = own_host_position(value) else {
        return false;
    };
    let mail = value.get(..at).is_some_and(|head| head.ends_with('@'));
    mail && !value.contains(ultar_brand::CONTACT_EMAIL)
}

fn error(path: &str, line: u32, key: Key, args: &[&str]) -> Finding {
    Finding::error(Rule::Brand, path, line, Message::new(key, args))
}

fn check_entry(path: &str, entry: &Entry, in_brand: bool, findings: &mut Vec<Finding>) {
    for (canonical, lower) in names() {
        if let Some(written) = misspelled(&entry.value, canonical, &lower) {
            let args = [entry.key.as_str(), written, canonical];
            findings.push(error(path, entry.line, Key::BrandSpelling, &args));
        }
    }
    if wrong_contact(&entry.value) {
        let args = [entry.key.as_str(), ultar_brand::CONTACT_EMAIL];
        findings.push(error(path, entry.line, Key::BrandValue, &args));
    }
    if ultar_brand::fact(&entry.key).is_some() {
        let args = [entry.key.as_str()];
        findings.push(error(path, entry.line, Key::BrandFactInCatalog, &args));
    } else if entry.key.starts_with(NAMESPACE) && !in_brand {
        let args = [entry.key.as_str()];
        findings.push(error(path, entry.line, Key::BrandKeyOutside, &args));
    }
    let alias = ALIASES.iter().find(|(key, _)| *key == entry.key);
    if let Some(expected) = alias.and_then(|(_, fact)| ultar_brand::fact(fact)) {
        if entry.value != expected {
            let args = [entry.key.as_str(), expected];
            findings.push(error(path, entry.line, Key::BrandValue, &args));
        }
    }
}

/// The brand catalogs must exist in every language and hold the same keys.
fn check_coverage(root: &Path, findings: &mut Vec<Finding>) -> Result<(), CheckError> {
    let mut catalogs = Vec::new();
    for lang in LANGS {
        let path = format!("{BRAND_LOCALES}/{lang}.ftl");
        match repo::read_optional(&root.join(&path))? {
            Some(text) => catalogs.push((lang, path, entries(&text))),
            None => findings.push(error(&path, 1, Key::BrandCatalogMissing, &[lang])),
        }
    }
    for (lang, path, own) in &catalogs {
        for (other_lang, _, other) in &catalogs {
            for entry in other.iter().filter(|e| !own.iter().any(|o| o.key == e.key)) {
                let args = [entry.key.as_str(), lang, other_lang];
                findings.push(error(path, 1, Key::BrandKeyMissing, &args));
            }
        }
        if *lang != LANGS[0] {
            continue;
        }
        for entry in own.iter().filter(|e| !e.value.chars().any(is_arabic)) {
            let args = [entry.key.as_str()];
            findings.push(error(path, entry.line, Key::BrandNotArabic, &args));
        }
    }
    findings.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    findings.dedup();
    Ok(())
}

/// Checks every catalog of the repository.
pub fn check(root: &Path) -> Result<Vec<Finding>, CheckError> {
    let mut findings = Vec::new();
    for path in repo::walk(root)?.catalogs {
        let text = repo::read_text(root, &path)?;
        let in_brand = path.starts_with(SOURCE_CRATE);
        for entry in entries(&text) {
            check_entry(&path, &entry, in_brand, &mut findings);
        }
    }
    if root.join(SOURCE_CRATE).is_dir() {
        check_coverage(root, &mut findings)?;
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_join_continuations_and_skip_comments() {
        let parsed = entries("# c\na = one\n    two\n\nb.c=x = y\nbroken\n= v\nbad key = v\n");
        assert_eq!(parsed.len(), 2);
        assert_eq!((parsed[0].key.as_str(), parsed[0].line), ("a", 2));
        assert_eq!(parsed[0].value, "one two");
        assert_eq!((parsed[1].key.as_str(), parsed[1].line), ("b.c", 5));
        assert_eq!(parsed[1].value, "x = y");
    }

    #[test]
    fn spelling_compares_every_occurrence() {
        let [(product, lower), _] = names();
        assert_eq!(misspelled("Install Ultar OS", product, &lower), None);
        assert_eq!(
            misspelled("Ultar OS / ULTAR OS", product, &lower),
            Some("ULTAR OS")
        );
        assert_eq!(misspelled("ultar os", product, &lower), Some("ultar os"));
        assert_eq!(misspelled("تثبيت Ultar OS", product, &lower), None);
        assert_eq!(misspelled("", product, &lower), None);
    }

    #[test]
    fn only_the_public_contact_address_is_accepted() {
        assert!(!wrong_contact("write to info@code-craft.tech"));
        assert!(wrong_contact("write to someone@code-craft.tech"));
        assert!(!wrong_contact("see https://code-craft.tech"));
        assert!(!wrong_contact("a@store.code-craft.tech"));
    }
}
