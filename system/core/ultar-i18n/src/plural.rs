//! CLDR plural rules for whole numbers.

use crate::lang::Lang;

/// A CLDR plural category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PluralCategory {
    /// Zero items (used by Arabic).
    Zero,
    /// Exactly one item.
    One,
    /// Exactly two items (Arabic dual).
    Two,
    /// A few items (Arabic: last two digits 03–10).
    Few,
    /// Many items (Arabic: last two digits 11–99).
    Many,
    /// Everything else; every language has this category.
    Other,
}

impl PluralCategory {
    /// All categories in CLDR order.
    pub const ALL: [PluralCategory; 6] = [
        PluralCategory::Zero,
        PluralCategory::One,
        PluralCategory::Two,
        PluralCategory::Few,
        PluralCategory::Many,
        PluralCategory::Other,
    ];

    /// The keyword used in catalogs (`zero`, `one`, `two`, `few`, `many`, `other`).
    pub const fn keyword(self) -> &'static str {
        match self {
            PluralCategory::Zero => "zero",
            PluralCategory::One => "one",
            PluralCategory::Two => "two",
            PluralCategory::Few => "few",
            PluralCategory::Many => "many",
            PluralCategory::Other => "other",
        }
    }

    /// Parses a catalog keyword; `None` if it is not a plural category.
    pub fn from_keyword(keyword: &str) -> Option<PluralCategory> {
        PluralCategory::ALL
            .iter()
            .copied()
            .find(|category| category.keyword() == keyword)
    }
}

/// Plural category of a whole number in the given language. The sign is ignored.
///
/// Arabic follows CLDR: 0 → zero, 1 → one, 2 → two, `n % 100` in 3..=10 → few,
/// `n % 100` in 11..=99 → many, otherwise other (100, 101, 102, 200 …).
pub fn category(lang: Lang, number: i64) -> PluralCategory {
    let n = number.unsigned_abs();
    match lang {
        Lang::Ar => arabic(n),
        Lang::En | Lang::De => {
            if n == 1 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
    }
}

fn arabic(n: u64) -> PluralCategory {
    match (n, n % 100) {
        (0, _) => PluralCategory::Zero,
        (1, _) => PluralCategory::One,
        (2, _) => PluralCategory::Two,
        (_, 3..=10) => PluralCategory::Few,
        (_, 11..=99) => PluralCategory::Many,
        _ => PluralCategory::Other,
    }
}

/// The categories a language distinguishes, in CLDR order.
pub const fn categories(lang: Lang) -> &'static [PluralCategory] {
    match lang {
        Lang::Ar => &PluralCategory::ALL,
        Lang::En | Lang::De => &[PluralCategory::One, PluralCategory::Other],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PluralCategory::*;

    #[test]
    fn arabic_rules_follow_cldr() {
        let cases = [
            (0, Zero),
            (1, One),
            (2, Two),
            (3, Few),
            (10, Few),
            (11, Many),
            (99, Many),
            (100, Other),
            (101, Other),
            (102, Other),
            (103, Few),
            (110, Few),
            (111, Many),
            (199, Many),
            (200, Other),
            (1000, Other),
            (1003, Few),
            (1_000_011, Many),
        ];
        for (n, expected) in cases {
            assert_eq!(category(Lang::Ar, n), expected, "n = {n}");
        }
    }

    #[test]
    fn english_and_german_rules() {
        for lang in [Lang::En, Lang::De] {
            assert_eq!(category(lang, 0), Other);
            assert_eq!(category(lang, 1), One);
            assert_eq!(category(lang, 2), Other);
            assert_eq!(category(lang, 101), Other);
        }
    }

    #[test]
    fn sign_is_ignored_and_extremes_do_not_overflow() {
        assert_eq!(category(Lang::Ar, -1), One);
        assert_eq!(category(Lang::Ar, -2), Two);
        assert_eq!(category(Lang::En, -1), One);
        assert_eq!(category(Lang::Ar, i64::MIN), Few);
        assert_eq!(category(Lang::Ar, i64::MAX), Few);
    }

    #[test]
    fn keywords_round_trip() {
        for category in PluralCategory::ALL {
            assert_eq!(
                PluralCategory::from_keyword(category.keyword()),
                Some(category)
            );
        }
        assert_eq!(PluralCategory::from_keyword("One"), None);
        assert_eq!(PluralCategory::from_keyword("several"), None);
    }

    #[test]
    fn category_lists() {
        assert_eq!(categories(Lang::Ar).len(), 6);
        assert_eq!(categories(Lang::En), &[One, Other]);
        assert_eq!(categories(Lang::De), &[One, Other]);
    }
}
