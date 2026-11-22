//! Arabic text utilities: normalisation for search.
//!
//! Search must find «أحمد» when the user types «احمد», and «مدرسة» when they type «مدرسه».
//! [`normalize_for_search`] folds both the stored text and the query to the same simple form.

use alloc::string::String;

use crate::numbers::western_digit;

/// `true` for Arabic combining marks (tashkeel/harakat, Quranic annotation signs, superscript alef).
pub fn is_tashkeel(c: char) -> bool {
    matches!(c as u32,
        0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC | 0x06DF..=0x06E4
        | 0x06E7 | 0x06E8 | 0x06EA..=0x06ED | 0x08D3..=0x08E1 | 0x08E3..=0x08FF)
}

/// The tatweel (kashida) character that only stretches words.
pub const TATWEEL: char = '\u{0640}';

/// `true` for characters that carry no meaning for search or sorting: tashkeel, tatweel,
/// zero-width joiners and direction marks.
pub fn is_ignorable(c: char) -> bool {
    is_tashkeel(c)
        || c == TATWEEL
        || matches!(c as u32, 0x200B..=0x200F | 0x202A..=0x202E | 0x2066..=0x2069 | 0x061C | 0xFEFF)
}

/// Presentation forms FE80–FEF4: (first code point, number of forms, base letter).
const PRESENTATION_FORMS: [(u32, u32, char); 36] = [
    (0xFE80, 1, 'ء'),
    (0xFE81, 2, 'آ'),
    (0xFE83, 2, 'أ'),
    (0xFE85, 2, 'ؤ'),
    (0xFE87, 2, 'إ'),
    (0xFE89, 4, 'ئ'),
    (0xFE8D, 2, 'ا'),
    (0xFE8F, 4, 'ب'),
    (0xFE93, 2, 'ة'),
    (0xFE95, 4, 'ت'),
    (0xFE99, 4, 'ث'),
    (0xFE9D, 4, 'ج'),
    (0xFEA1, 4, 'ح'),
    (0xFEA5, 4, 'خ'),
    (0xFEA9, 2, 'د'),
    (0xFEAB, 2, 'ذ'),
    (0xFEAD, 2, 'ر'),
    (0xFEAF, 2, 'ز'),
    (0xFEB1, 4, 'س'),
    (0xFEB5, 4, 'ش'),
    (0xFEB9, 4, 'ص'),
    (0xFEBD, 4, 'ض'),
    (0xFEC1, 4, 'ط'),
    (0xFEC5, 4, 'ظ'),
    (0xFEC9, 4, 'ع'),
    (0xFECD, 4, 'غ'),
    (0xFED1, 4, 'ف'),
    (0xFED5, 4, 'ق'),
    (0xFED9, 4, 'ك'),
    (0xFEDD, 4, 'ل'),
    (0xFEE1, 4, 'م'),
    (0xFEE5, 4, 'ن'),
    (0xFEE9, 4, 'ه'),
    (0xFEED, 2, 'و'),
    (0xFEEF, 2, 'ى'),
    (0xFEF1, 4, 'ي'),
];

/// Lam-alef ligatures FEF5–FEFC: (first code point, alef that follows the lam).
const LAM_ALEF: [(u32, char); 4] = [(0xFEF5, 'آ'), (0xFEF7, 'أ'), (0xFEF9, 'إ'), (0xFEFB, 'ا')];

/// Replaces an Arabic presentation form (as produced by old software and PDFs) with its base
/// letter(s). Returns the letter and, for lam-alef ligatures, the alef that follows.
pub fn decompose_presentation_form(c: char) -> (char, Option<char>) {
    let code = c as u32;
    if !(0xFE80..=0xFEFC).contains(&code) {
        return (c, None);
    }
    if let Some((_, alef)) = LAM_ALEF
        .iter()
        .find(|(first, _)| (*first..first + 2).contains(&code))
    {
        return ('ل', Some(*alef));
    }
    PRESENTATION_FORMS
        .iter()
        .find(|(first, count, _)| (*first..first + count).contains(&code))
        .map_or((c, None), |(_, _, base)| (*base, None))
}

/// Folds one Arabic letter to its search form: alef forms → ا, ة → ه, ى/ی/ئ → ي, ؤ → و, ک → ك.
pub fn fold_letter(c: char) -> char {
    match c {
        'أ' | 'إ' | 'آ' | 'ٱ' | 'ٲ' | 'ٳ' | 'ٵ' => 'ا',
        'ة' | 'ۀ' | 'ہ' | 'ە' | 'ھ' => 'ه',
        'ى' | 'ی' | 'ئ' | 'ې' | 'ے' => 'ي',
        'ؤ' => 'و',
        'ک' => 'ك',
        other => other,
    }
}

/// Normalises text for searching and matching.
///
/// * strips tashkeel, tatweel and invisible direction/joiner marks,
/// * replaces presentation forms (ﻻ → لا),
/// * unifies alef forms (أ إ آ ٱ → ا), ة → ه, ى → ي, hamza carriers ؤ → و and ئ → ي,
/// * folds Persian/Urdu variants (ک → ك, ی → ي),
/// * converts Arabic-Indic and Persian digits to 0–9,
/// * lower-cases Latin (and other cased) letters.
///
/// Apply it to both the text and the query, then compare or use `str::contains`.
pub fn normalize_for_search(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for original in text.chars() {
        if is_ignorable(original) {
            continue;
        }
        let (first, second) = decompose_presentation_form(original);
        for c in [Some(first), second].into_iter().flatten() {
            let folded = fold_letter(c);
            match western_digit(folded) {
                Some(digit) => out.push(digit),
                None => out.extend(folded.to_lowercase()),
            }
        }
    }
    out
}

/// `true` if `text` contains `query` after both were normalised with [`normalize_for_search`].
///
/// An empty query matches everything.
pub fn search_matches(text: &str, query: &str) -> bool {
    normalize_for_search(text).contains(&normalize_for_search(query))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tashkeel_and_tatweel() {
        assert_eq!(normalize_for_search("مُحَمَّدٌ"), "محمد");
        assert_eq!(normalize_for_search("الـــسلام"), "السلام");
        assert_eq!(normalize_for_search("رَحْمٰن"), "رحمن");
        assert_eq!(normalize_for_search("a\u{200F}b\u{200D}c\u{FEFF}"), "abc");
    }

    #[test]
    fn unifies_letters() {
        assert_eq!(
            normalize_for_search("أحمد إبراهيم آدم ٱلله"),
            "احمد ابراهيم ادم الله"
        );
        assert_eq!(normalize_for_search("مدرسة"), "مدرسه");
        assert_eq!(normalize_for_search("مستشفى"), "مستشفي");
        assert_eq!(normalize_for_search("مسؤول"), "مسوول");
        assert_eq!(normalize_for_search("رئيس"), "رييس");
        assert_eq!(normalize_for_search("ماء"), "ماء");
        assert_eq!(normalize_for_search("کتاب فارسی"), "كتاب فارسي");
    }

    #[test]
    fn folds_latin_case_and_digits() {
        assert_eq!(
            normalize_for_search("Ultar OS Größe ÄÖÜ"),
            "ultar os größe äöü"
        );
        assert_eq!(normalize_for_search("غرفة ١٢ / ۳۴"), "غرفه 12 / 34");
        assert_eq!(normalize_for_search(""), "");
    }

    #[test]
    fn presentation_forms_decompose() {
        assert_eq!(normalize_for_search("ﻻ"), "لا");
        assert_eq!(normalize_for_search("ﻷ"), "لا");
        assert_eq!(normalize_for_search("ﺳﻼﻡ"), "سلام");
        assert_eq!(decompose_presentation_form('ﺏ'), ('ب', None));
        assert_eq!(decompose_presentation_form('ﻵ'), ('ل', Some('آ')));
        assert_eq!(decompose_presentation_form('\u{FEFC}'), ('ل', Some('ا')));
        assert_eq!(decompose_presentation_form('\u{FEF4}'), ('ي', None));
        assert_eq!(decompose_presentation_form('ب'), ('ب', None));
        assert_eq!(decompose_presentation_form('x'), ('x', None));
    }

    #[test]
    fn presentation_table_is_contiguous() {
        let mut next = 0xFE80;
        for (first, count, _) in PRESENTATION_FORMS {
            assert_eq!(first, next);
            next = first + count;
        }
        assert_eq!(next, 0xFEF5);
    }

    #[test]
    fn character_classes() {
        assert!(is_tashkeel('\u{064E}'));
        assert!(is_tashkeel('\u{0670}'));
        assert!(!is_tashkeel('ا'));
        assert!(is_ignorable(TATWEEL));
        assert!(is_ignorable('\u{200C}'));
        assert!(!is_ignorable(' '));
        assert_eq!(fold_letter('ة'), 'ه');
        assert_eq!(fold_letter('ب'), 'ب');
    }

    #[test]
    fn search_matching() {
        assert!(search_matches("مُستشفى الأَمل", "مستشفي الامل"));
        assert!(search_matches("Ultar إعدادات", "اعدادات"));
        assert!(search_matches("Settings", "SETT"));
        assert!(search_matches("anything", ""));
        assert!(!search_matches("مدرسة", "جامعة"));
    }
}
