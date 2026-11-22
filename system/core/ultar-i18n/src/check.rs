//! Completeness checks between catalogs: missing keys, differing placeholders, missing plural forms.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::catalog::Catalog;
use crate::lang::Lang;
use crate::plural::{self, PluralCategory};

/// A message whose placeholders differ from the reference catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceholderMismatch {
    /// The message key.
    pub key: String,
    /// Placeholders the reference uses but the other catalog lacks.
    pub missing: Vec<String>,
    /// Placeholders the other catalog uses but the reference does not know.
    pub unexpected: Vec<String>,
}

/// A message with plural variants that lacks forms its language needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluralGap {
    /// The message key.
    pub key: String,
    /// The plural categories without a variant.
    pub missing: Vec<PluralCategory>,
}

/// Result of comparing one catalog with the reference catalog.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Comparison {
    /// Keys of the reference that the other catalog lacks.
    pub missing: Vec<String>,
    /// Keys of the other catalog that the reference lacks.
    pub extra: Vec<String>,
    /// Messages present in both whose placeholders differ.
    pub placeholders: Vec<PlaceholderMismatch>,
}

impl Comparison {
    /// `true` if the catalogs have the same keys and placeholders.
    pub fn is_clean(&self) -> bool {
        self.missing.is_empty() && self.extra.is_empty() && self.placeholders.is_empty()
    }

    /// Total number of findings.
    pub fn problem_count(&self) -> usize {
        self.missing.len() + self.extra.len() + self.placeholders.len()
    }
}

/// Keys of `reference` that `other` does not have, sorted.
///
/// Call it in both directions to also find keys that only `other` has.
pub fn missing_keys(reference: &Catalog, other: &Catalog) -> Vec<String> {
    reference
        .keys()
        .filter(|key| !other.contains(key))
        .map(ToString::to_string)
        .collect()
}

/// Messages present in both catalogs whose sets of placeholders differ, sorted by key.
///
/// Selection variables count as placeholders, so a plural message must use the same variable name
/// in every language.
pub fn placeholder_mismatches(reference: &Catalog, other: &Catalog) -> Vec<PlaceholderMismatch> {
    let mut found = Vec::new();
    for (key, message) in reference.iter() {
        let Some(translated) = other.get(key) else {
            continue;
        };
        let expected = message.placeholders();
        let actual = translated.placeholders();
        if expected == actual {
            continue;
        }
        found.push(PlaceholderMismatch {
            key: key.to_string(),
            missing: expected
                .difference(&actual)
                .map(ToString::to_string)
                .collect(),
            unexpected: actual
                .difference(&expected)
                .map(ToString::to_string)
                .collect(),
        });
    }
    found
}

/// Messages of `catalog` that select by plural category but lack a form that `lang` needs
/// (all six for Arabic, `one` and `other` for English and German).
pub fn incomplete_plurals(catalog: &Catalog, lang: Lang) -> Vec<PluralGap> {
    let mut found = Vec::new();
    for (key, message) in catalog.iter() {
        let present = message.plural_categories();
        if present.is_empty() {
            continue;
        }
        let missing: Vec<PluralCategory> = plural::categories(lang)
            .iter()
            .copied()
            .filter(|category| !present.contains(category))
            .collect();
        if !missing.is_empty() {
            found.push(PluralGap {
                key: key.to_string(),
                missing,
            });
        }
    }
    found
}

/// Compares `other` with `reference` in both directions.
pub fn compare_catalogs(reference: &Catalog, other: &Catalog) -> Comparison {
    Comparison {
        missing: missing_keys(reference, other),
        extra: missing_keys(other, reference),
        placeholders: placeholder_mismatches(reference, other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(source: &str) -> Catalog {
        Catalog::parse(source).unwrap()
    }

    #[test]
    fn finds_missing_keys_in_both_directions() {
        let reference = catalog("a = 1\nb = 2\nc = 3\n");
        let other = catalog("b = 2\nd = 4\n");
        assert_eq!(missing_keys(&reference, &other), ["a", "c"]);
        assert_eq!(missing_keys(&other, &reference), ["d"]);
        assert!(missing_keys(&reference, &reference).is_empty());
        assert!(missing_keys(&Catalog::new(), &other).is_empty());
    }

    #[test]
    fn finds_placeholder_differences() {
        let reference =
            catalog("a = {x} {y}\nb = {n -> [one] one *[other] {n}}\nc = plain\nd = {x}\n");
        let other = catalog("a = {y} {z}\nb = {n -> *[other] many}\nc = {extra}\n");
        let found = placeholder_mismatches(&reference, &other);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].key, "a");
        assert_eq!(found[0].missing, ["x"]);
        assert_eq!(found[0].unexpected, ["z"]);
        assert_eq!(found[1].key, "c");
        assert!(found[1].missing.is_empty());
        assert_eq!(found[1].unexpected, ["extra"]);
        assert!(placeholder_mismatches(&reference, &reference).is_empty());
    }

    #[test]
    fn finds_incomplete_plurals() {
        let source = "a = {n -> [one] x *[other] y}\nb = plain {n}\nc = {k -> [wifi] w *[lan] l}\n";
        let parsed = catalog(source);
        assert!(incomplete_plurals(&parsed, Lang::En).is_empty());
        assert!(incomplete_plurals(&parsed, Lang::De).is_empty());
        let gaps = incomplete_plurals(&parsed, Lang::Ar);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].key, "a");
        assert_eq!(
            gaps[0].missing,
            [
                PluralCategory::Zero,
                PluralCategory::Two,
                PluralCategory::Few,
                PluralCategory::Many
            ]
        );
        let only_other = catalog("a = {n -> [few] x *[many] y}\n");
        assert_eq!(
            incomplete_plurals(&only_other, Lang::En)[0].missing,
            [PluralCategory::One, PluralCategory::Other]
        );
    }

    #[test]
    fn comparison_summarises() {
        let reference = catalog("a = {x}\nb = 2\n");
        let other = catalog("a = {y}\nc = 3\n");
        let result = compare_catalogs(&reference, &other);
        assert!(!result.is_clean());
        assert_eq!(result.problem_count(), 3);
        assert_eq!(result.missing, ["b"]);
        assert_eq!(result.extra, ["c"]);
        let same = compare_catalogs(&reference, &reference);
        assert!(same.is_clean());
        assert_eq!(same.problem_count(), 0);
        assert_eq!(same, Comparison::default());
    }
}
