//! Direction detection for text: does it contain right-to-left characters, and which way does it start.
//!
//! This is the *first strong character* rule of the Unicode Bidirectional Algorithm (rules P2/P3),
//! enough to pick the base direction of a line. Full reordering of mixed text is done by the text
//! layout in `ultar-ui`.

use crate::lang::Direction;

/// `true` if `c` is a strong right-to-left character (Arabic, Hebrew, Syriac, Thaana, N'Ko …).
///
/// Arabic-Indic digits, tashkeel and punctuation are not strong and return `false`.
pub fn is_rtl_char(c: char) -> bool {
    matches!(c as u32,
        0x05BE | 0x05C0 | 0x05C3 | 0x05C6 | 0x05D0..=0x05F4
        | 0x0608 | 0x060B | 0x060D | 0x061B..=0x064A
        | 0x066D..=0x066F | 0x0671..=0x06D5 | 0x06E5 | 0x06E6 | 0x06EE | 0x06EF
        | 0x06FA..=0x070D | 0x0710 | 0x0712..=0x072F | 0x074D..=0x07A5 | 0x07B1
        | 0x07C0..=0x07EA | 0x07F4 | 0x07F5 | 0x07FA
        | 0x0800..=0x0815 | 0x0840..=0x0858 | 0x0860..=0x086A | 0x08A0..=0x08C9
        | 0x200F
        | 0xFB1D | 0xFB1F..=0xFB28 | 0xFB2A..=0xFD3D | 0xFD50..=0xFDFC | 0xFE70..=0xFEFC
        | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF)
}

/// `true` if `c` is a strong left-to-right character (a letter of a left-to-right script).
pub fn is_ltr_char(c: char) -> bool {
    c == '\u{200E}' || (c.is_alphabetic() && !is_rtl_char(c))
}

/// `true` if the text contains at least one strong right-to-left character.
pub fn contains_rtl(text: &str) -> bool {
    text.chars().any(is_rtl_char)
}

/// Direction of the first strong character, or `None` if the text has none (digits, symbols, empty).
pub fn first_strong_direction(text: &str) -> Option<Direction> {
    text.chars().find_map(|c| {
        if is_rtl_char(c) {
            Some(Direction::Rtl)
        } else if is_ltr_char(c) {
            Some(Direction::Ltr)
        } else {
            None
        }
    })
}

/// Base direction of a text: its first strong character decides, otherwise `fallback`
/// (normally the direction of the interface language).
pub fn direction_of(text: &str, fallback: Direction) -> Direction {
    first_strong_direction(text).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtl_detection() {
        assert!(contains_rtl("مرحبا"));
        assert!(contains_rtl("Ultar نظام"));
        assert!(contains_rtl("שלום"));
        assert!(contains_rtl("ﻻ"));
        assert!(!contains_rtl("Ultar OS 2"));
        assert!(!contains_rtl(""));
        assert!(!contains_rtl("١٢٣"));
        assert!(!contains_rtl("\u{064E}\u{0651}"));
        assert!(!contains_rtl("، . !"));
    }

    #[test]
    fn first_strong() {
        assert_eq!(first_strong_direction("مرحبا World"), Some(Direction::Rtl));
        assert_eq!(first_strong_direction("Hello عالم"), Some(Direction::Ltr));
        assert_eq!(first_strong_direction("123 مرحبا"), Some(Direction::Rtl));
        assert_eq!(first_strong_direction("(1) – Größe"), Some(Direction::Ltr));
        assert_eq!(first_strong_direction("٣ + 4 = 7"), None);
        assert_eq!(first_strong_direction(""), None);
        assert_eq!(first_strong_direction("\u{200F}abc"), Some(Direction::Rtl));
        assert_eq!(
            first_strong_direction("\u{200E}مرحبا"),
            Some(Direction::Ltr)
        );
    }

    #[test]
    fn fallback_applies_without_strong_characters() {
        assert_eq!(direction_of("12:30", Direction::Rtl), Direction::Rtl);
        assert_eq!(direction_of("12:30", Direction::Ltr), Direction::Ltr);
        assert_eq!(direction_of("abc", Direction::Rtl), Direction::Ltr);
        assert_eq!(direction_of("أبجد", Direction::Ltr), Direction::Rtl);
    }

    #[test]
    fn character_classes() {
        assert!(is_rtl_char('ع'));
        assert!(is_rtl_char('پ'));
        assert!(!is_rtl_char('٣'));
        assert!(!is_rtl_char('۳'));
        assert!(!is_rtl_char('a'));
        assert!(is_ltr_char('a'));
        assert!(is_ltr_char('ß'));
        assert!(is_ltr_char('Ж'));
        assert!(!is_ltr_char('ع'));
        assert!(!is_ltr_char('5'));
        assert!(!is_ltr_char(' '));
    }
}
