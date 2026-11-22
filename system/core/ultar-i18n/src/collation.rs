//! Sorting text in Arabic alphabetical order, mixed with Latin.
//!
//! Order of groups: spaces and punctuation, digits, Arabic letters, Latin letters, everything else.
//! Arabic comes before Latin as in the CLDR Arabic collation. Tashkeel, tatweel and invisible marks
//! are ignored. Hamza forms sort with their carrier (أ إ آ with ا), differing only on the second
//! level; Latin case and diacritics likewise (`a` < `A` < `ä`, all before `b`).

use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::arabic::{decompose_presentation_form, is_ignorable};
use crate::numbers::western_digit;

/// Arabic alphabet in sorting order. Letters in one group share the primary weight; their position
/// in the group is the secondary weight. Persian letters sit after the letter they derive from.
const ARABIC_ORDER: [&str; 36] = [
    "ء",
    "اآأإٱ",
    "ب",
    "پ",
    "ت",
    "ث",
    "ج",
    "چ",
    "ح",
    "خ",
    "د",
    "ذ",
    "ر",
    "ز",
    "ژ",
    "س",
    "ش",
    "ص",
    "ض",
    "ط",
    "ظ",
    "ع",
    "غ",
    "ف",
    "ڤ",
    "ق",
    "كک",
    "گ",
    "ل",
    "م",
    "ن",
    "ه",
    "ة",
    "وؤ",
    "ى",
    "يیئ",
];

/// Latin letters with diacritics and the base letter they sort with.
const LATIN_FOLDS: [(&str, char); 9] = [
    ("àáâãäå", 'a'),
    ("ç", 'c'),
    ("èéêë", 'e'),
    ("ìíîï", 'i'),
    ("ñ", 'n'),
    ("òóôõö", 'o'),
    ("ùúûü", 'u'),
    ("ýÿ", 'y'),
    ("ß", 's'),
];

const GROUP_PUNCTUATION: u32 = 1 << 24;
const GROUP_DIGIT: u32 = 2 << 24;
const GROUP_ARABIC: u32 = 3 << 24;
const GROUP_LATIN: u32 = 4 << 24;
const GROUP_OTHER: u32 = 5 << 24;

/// A sort key: compare two keys with `<`, `cmp` or use them in `sort_by_key`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CollationKey(Vec<u32>);

impl CollationKey {
    /// The raw weights (primary weights, a zero, then secondary weights).
    pub fn weights(&self) -> &[u32] {
        &self.0
    }
}

fn arabic_weight(c: char) -> Option<(u32, u32)> {
    ARABIC_ORDER
        .iter()
        .zip(1u32..)
        .find_map(|(group, primary)| {
            group
                .chars()
                .zip(0u32..)
                .find(|(letter, _)| *letter == c)
                .map(|(_, secondary)| (GROUP_ARABIC | primary, secondary))
        })
}

fn latin_weight(c: char) -> Option<(u32, u32)> {
    let lower = c.to_lowercase().next().unwrap_or(c);
    let upper = u32::from(lower != c);
    if lower.is_ascii_lowercase() {
        return Some((GROUP_LATIN | (lower as u32 - 'a' as u32), upper));
    }
    LATIN_FOLDS.iter().find_map(|(variants, base)| {
        variants
            .chars()
            .zip(1u32..)
            .find(|(variant, _)| *variant == lower)
            .map(|(_, index)| (GROUP_LATIN | (*base as u32 - 'a' as u32), index * 2 + upper))
    })
}

fn weight(c: char) -> (u32, u32) {
    if let Some(digit) = c
        .to_digit(10)
        .or_else(|| western_digit(c).and_then(|d| d.to_digit(10)))
    {
        return (GROUP_DIGIT | digit, u32::from(!c.is_ascii_digit()));
    }
    if let Some(found) = arabic_weight(c).or_else(|| latin_weight(c)) {
        return found;
    }
    let code = c as u32;
    if c.is_alphanumeric() {
        (GROUP_OTHER | code, 0)
    } else {
        (GROUP_PUNCTUATION | code, 0)
    }
}

/// Builds the sort key of a text.
pub fn collation_key(text: &str) -> CollationKey {
    let mut primary = Vec::with_capacity(text.len());
    let mut secondary = Vec::with_capacity(text.len());
    for original in text.chars().filter(|c| !is_ignorable(*c)) {
        let (first, second) = decompose_presentation_form(original);
        for c in [Some(first), second].into_iter().flatten() {
            let (p, s) = weight(c);
            primary.push(p);
            secondary.push(s);
        }
    }
    primary.push(0);
    primary.append(&mut secondary);
    CollationKey(primary)
}

/// Compares two texts in Arabic alphabetical order mixed with Latin.
///
/// Texts with equal keys (they differ only in ignored marks) are ordered by code points, so the
/// order is total and stable for sorting.
pub fn compare(a: &str, b: &str) -> Ordering {
    collation_key(a)
        .cmp(&collation_key(b))
        .then_with(|| a.cmp(b))
}

/// Sorts texts in place with [`compare`].
pub fn sort<S: AsRef<str>>(items: &mut [S]) {
    items.sort_by(|a, b| compare(a.as_ref(), b.as_ref()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn arabic_alphabet_is_in_order() {
        let alphabet = [
            "ا", "ب", "ت", "ث", "ج", "ح", "خ", "د", "ذ", "ر", "ز", "س", "ش", "ص", "ض", "ط", "ظ",
            "ع", "غ", "ف", "ق", "ك", "ل", "م", "ن", "ه", "و", "ي",
        ];
        for pair in alphabet.windows(2) {
            assert_eq!(
                compare(pair[0], pair[1]),
                Ordering::Less,
                "{} < {}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn hamza_forms_sort_with_their_carrier() {
        let mut names = vec!["باسم", "إبراهيم", "أحمد", "آدم", "ثامر", "ابتسام", "أنس"];
        sort(&mut names);
        assert_eq!(
            names,
            ["ابتسام", "إبراهيم", "أحمد", "آدم", "أنس", "باسم", "ثامر"]
        );
        assert_eq!(compare("ا", "أ"), Ordering::Less);
        assert_eq!(compare("أب", "ات"), Ordering::Less);
    }

    #[test]
    fn marks_are_ignored_on_the_first_level() {
        assert_eq!(collation_key("مُحَمَّد"), collation_key("محمد"));
        assert_eq!(collation_key("الـسلام"), collation_key("السلام"));
        assert_ne!(compare("مُحَمَّد", "محمد"), Ordering::Equal);
        assert_eq!(compare("محمد", "محمد"), Ordering::Equal);
        assert_eq!(compare("مُحمد", "محمود"), Ordering::Less);
        assert_eq!(collation_key("ﻻ"), collation_key("لا"));
    }

    #[test]
    fn groups_order_digits_arabic_latin() {
        let mut items = vec![
            "zebra",
            "Ultar",
            "تطبيق",
            "9",
            "١٠",
            "apple",
            " فراغ",
            "إعدادات",
            "中",
        ];
        sort(&mut items);
        assert_eq!(
            items,
            [
                " فراغ",
                "١٠",
                "9",
                "إعدادات",
                "تطبيق",
                "apple",
                "Ultar",
                "zebra",
                "中"
            ]
        );
    }

    #[test]
    fn latin_case_and_diacritics() {
        assert_eq!(compare("apple", "Apple"), Ordering::Less);
        assert_eq!(compare("Apple", "banana"), Ordering::Less);
        assert_eq!(compare("Äpfel", "Azur"), Ordering::Less);
        assert_eq!(compare("Zebra", "apple"), Ordering::Greater);
        assert_eq!(compare("über", "uhr"), Ordering::Less);
        assert_eq!(compare("a", "ä"), Ordering::Less);
        assert_eq!(compare("Straße", "Strasse"), Ordering::Less);
    }

    #[test]
    fn prefixes_and_empty_text() {
        assert_eq!(compare("", ""), Ordering::Equal);
        assert_eq!(compare("", "ا"), Ordering::Less);
        assert_eq!(compare("كتاب", "كتابة"), Ordering::Less);
        assert_eq!(compare("ab", "abc"), Ordering::Less);
        assert_eq!(collation_key("").weights(), &[0]);
    }

    #[test]
    fn persian_letters_and_digits() {
        assert_eq!(compare("ب", "پ"), Ordering::Less);
        assert_eq!(compare("پ", "ت"), Ordering::Less);
        assert_eq!(compare("ك", "گ"), Ordering::Less);
        assert_eq!(compare("٢", "3"), Ordering::Less);
        assert_eq!(compare("2", "٢"), Ordering::Less);
        assert_eq!(compare("ه", "ة"), Ordering::Less);
        assert_eq!(compare("ة", "و"), Ordering::Less);
    }

    #[test]
    fn order_is_total_and_antisymmetric() {
        let items = ["", "ا", "أ", "ب", "a", "A", "ä", "1", "١", "!", "مُ", "م"];
        for a in items {
            for b in items {
                assert_eq!(compare(a, b), compare(b, a).reverse(), "{a:?} vs {b:?}");
            }
        }
    }
}
