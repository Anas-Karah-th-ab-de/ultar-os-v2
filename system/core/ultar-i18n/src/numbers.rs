//! Numbers: Ultar OS always shows Western digits 0–9, also in Arabic.
//!
//! Input may still arrive with Arabic-Indic (٠–٩) or Persian (۰–۹) digits; the functions here fold
//! it to 0–9 before parsing, and format numbers with the grouping of each language.

use alloc::string::{String, ToString};
use core::fmt;

use crate::lang::Lang;

/// Error of [`parse_int`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberError {
    /// The input contains no digits.
    Empty,
    /// The input contains something that is not part of a whole number.
    Invalid,
    /// The number does not fit into 64 bits.
    Overflow,
}

impl NumberError {
    /// Message key that describes this error in the catalogs.
    pub const fn message_key(self) -> &'static str {
        match self {
            NumberError::Empty => "i18n.number.empty",
            NumberError::Invalid => "i18n.number.invalid",
            NumberError::Overflow => "i18n.number.overflow",
        }
    }
}

impl fmt::Display for NumberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            NumberError::Empty => "no digits",
            NumberError::Invalid => "not a whole number",
            NumberError::Overflow => "number out of range",
        })
    }
}

impl core::error::Error for NumberError {}

/// Group and decimal separators of a language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Separators {
    /// Separator between groups of three digits.
    pub group: char,
    /// Separator before the fraction.
    pub decimal: char,
}

/// Separators of a language. Arabic with Western digits uses `1,234.5` like English (CLDR `ar`, `latn`).
pub const fn separators(lang: Lang) -> Separators {
    match lang {
        Lang::Ar | Lang::En => Separators {
            group: ',',
            decimal: '.',
        },
        Lang::De => Separators {
            group: '.',
            decimal: ',',
        },
    }
}

/// Maps an Arabic-Indic or Persian digit to its Western digit; other characters are returned as `None`.
pub fn western_digit(c: char) -> Option<char> {
    let offset = match c {
        '\u{0660}'..='\u{0669}' => c as u32 - 0x0660,
        '\u{06F0}'..='\u{06F9}' => c as u32 - 0x06F0,
        _ => return None,
    };
    char::from_digit(offset, 10)
}

/// Replaces Arabic-Indic (٠–٩) and Persian (۰–۹) digits with 0–9; everything else stays.
pub fn to_western_digits(text: &str) -> String {
    text.chars()
        .map(|c| western_digit(c).unwrap_or(c))
        .collect()
}

/// Prepares typed number input: digits become 0–9, `٫` becomes `.`, `٬` (Arabic thousands separator)
/// and invisible direction marks are removed, `٪` becomes `%`, `−` becomes `-`.
pub fn normalize_number_input(text: &str) -> String {
    text.trim()
        .chars()
        .filter_map(|c| match c {
            '\u{066B}' => Some('.'),
            '\u{066A}' => Some('%'),
            '\u{2212}' => Some('-'),
            '\u{066C}' | '\u{200E}' | '\u{200F}' | '\u{061C}' => None,
            other => Some(western_digit(other).unwrap_or(other)),
        })
        .collect()
}

/// Parses a whole number typed with Western, Arabic-Indic or Persian digits.
pub fn parse_int(text: &str) -> Result<i64, NumberError> {
    let cleaned = normalize_number_input(text);
    let digits = cleaned.strip_prefix(['-', '+']).unwrap_or(&cleaned);
    if digits.is_empty() {
        return Err(NumberError::Empty);
    }
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(NumberError::Invalid);
    }
    cleaned.parse::<i64>().map_err(|_| NumberError::Overflow)
}

/// Inserts `separator` between groups of three digits, counted from the right.
///
/// `digits` is expected to hold only digits; other input is grouped by characters all the same.
pub fn group_digits(digits: &str, separator: char) -> String {
    let count = digits.chars().count();
    let mut out = String::with_capacity(digits.len() + count / 3);
    for (index, c) in digits.chars().enumerate() {
        if index > 0 && (count - index).is_multiple_of(3) {
            out.push(separator);
        }
        out.push(c);
    }
    out
}

/// Formats an unsigned whole number with the language's grouping, e.g. `1,234,567` / `1.234.567`.
pub fn format_uint(value: u64, lang: Lang) -> String {
    group_digits(&value.to_string(), separators(lang).group)
}

/// Formats a whole number with the language's grouping. Negative numbers get a leading `-`.
pub fn format_int(value: i64, lang: Lang) -> String {
    let grouped = format_uint(value.unsigned_abs(), lang);
    if value < 0 {
        let mut out = String::with_capacity(grouped.len() + 1);
        out.push('-');
        out.push_str(&grouped);
        out
    } else {
        grouped
    }
}

/// Formats a fixed-point number: `scaled` is the value multiplied by `10^decimals`.
///
/// `format_fixed(123456, 2, Lang::De)` gives `1.234,56`. No floating point is involved, so the kernel
/// can use it. `decimals` above 18 are treated as 18.
pub fn format_fixed(scaled: i64, decimals: u8, lang: Lang) -> String {
    let decimals = u32::from(decimals.min(18));
    let divisor = 10u64.pow(decimals);
    let magnitude = scaled.unsigned_abs();
    let mut out = String::new();
    if scaled < 0 {
        out.push('-');
    }
    out.push_str(&format_uint(magnitude / divisor, lang));
    if decimals > 0 {
        out.push(separators(lang).decimal);
        let fraction = (magnitude % divisor).to_string();
        for _ in fraction.len()..decimals as usize {
            out.push('0');
        }
        out.push_str(&fraction);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_are_folded() {
        assert_eq!(to_western_digits("٠١٢٣٤٥٦٧٨٩"), "0123456789");
        assert_eq!(to_western_digits("۰۱۲۳۴۵۶۷۸۹"), "0123456789");
        assert_eq!(to_western_digits("هاتف ٠٩٣ – 12 – ۴"), "هاتف 093 – 12 – 4");
        assert_eq!(to_western_digits(""), "");
        assert_eq!(western_digit('٧'), Some('7'));
        assert_eq!(western_digit('۷'), Some('7'));
        assert_eq!(western_digit('7'), None);
        assert_eq!(western_digit('a'), None);
    }

    #[test]
    fn input_is_normalized() {
        assert_eq!(normalize_number_input(" ١٬٢٣٤٫٥٪ "), "1234.5%");
        assert_eq!(normalize_number_input("\u{200F}−٥"), "-5");
        assert_eq!(normalize_number_input("abc"), "abc");
    }

    #[test]
    fn whole_numbers_parse() {
        assert_eq!(parse_int("١٢٣"), Ok(123));
        assert_eq!(parse_int(" ۴۵ "), Ok(45));
        assert_eq!(parse_int("-٧"), Ok(-7));
        assert_eq!(parse_int("+7"), Ok(7));
        assert_eq!(parse_int("١٬٠٠٠"), Ok(1000));
        assert_eq!(parse_int("-9223372036854775808"), Ok(i64::MIN));
        assert_eq!(parse_int(""), Err(NumberError::Empty));
        assert_eq!(parse_int("-"), Err(NumberError::Empty));
        assert_eq!(parse_int("12a"), Err(NumberError::Invalid));
        assert_eq!(parse_int("1.5"), Err(NumberError::Invalid));
        assert_eq!(parse_int("--1"), Err(NumberError::Invalid));
        assert_eq!(parse_int("9223372036854775808"), Err(NumberError::Overflow));
    }

    #[test]
    fn errors_have_keys_and_text() {
        for error in [
            NumberError::Empty,
            NumberError::Invalid,
            NumberError::Overflow,
        ] {
            assert!(error.message_key().starts_with("i18n.number."));
            assert!(!alloc::format!("{error}").is_empty());
        }
    }

    #[test]
    fn grouping() {
        assert_eq!(group_digits("", ','), "");
        assert_eq!(group_digits("1", ','), "1");
        assert_eq!(group_digits("123", ','), "123");
        assert_eq!(group_digits("1234", ','), "1,234");
        assert_eq!(group_digits("123456", ' '), "123 456");
        assert_eq!(format_uint(0, Lang::Ar), "0");
        assert_eq!(format_uint(1_234_567, Lang::Ar), "1,234,567");
        assert_eq!(
            format_uint(u64::MAX, Lang::En),
            "18,446,744,073,709,551,615"
        );
        assert_eq!(format_int(-1_234_567, Lang::De), "-1.234.567");
        assert_eq!(format_int(999, Lang::En), "999");
        assert_eq!(format_int(i64::MIN, Lang::En), "-9,223,372,036,854,775,808");
        assert_eq!(
            separators(Lang::De),
            Separators {
                group: '.',
                decimal: ','
            }
        );
    }

    #[test]
    fn fixed_point() {
        assert_eq!(format_fixed(123_456, 2, Lang::De), "1.234,56");
        assert_eq!(format_fixed(123_456, 2, Lang::Ar), "1,234.56");
        assert_eq!(format_fixed(5, 2, Lang::En), "0.05");
        assert_eq!(format_fixed(-5, 1, Lang::En), "-0.5");
        assert_eq!(format_fixed(42, 0, Lang::En), "42");
        assert_eq!(format_fixed(1_000, 3, Lang::En), "1.000");
        assert_eq!(
            format_fixed(i64::MIN, 30, Lang::En),
            "-9.223372036854775808"
        );
    }

    #[test]
    fn output_never_contains_arabic_digits() {
        for lang in Lang::ALL {
            let text = format_int(9_876_543_210, lang);
            assert!(text
                .chars()
                .all(|c| c.is_ascii_digit() || c == ',' || c == '.'));
        }
    }
}
