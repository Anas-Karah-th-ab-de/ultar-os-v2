//! Text normalisation for catalogue search and validation of client-supplied identifiers.
//!
//! Search is tolerant for Arabic: diacritics (tashkeel) and tatweel are ignored, the alef, yaa,
//! waw and taa-marbuta variants are folded. Latin text is lower-cased.

/// Longest accepted search query, in characters.
pub const MAX_QUERY_CHARS: usize = 100;

/// Normalises text for matching: lower case, Arabic folding, single spaces.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for c in text.chars() {
        let Some(folded) = fold_char(c) else { continue };
        if folded.is_whitespace() || folded.is_control() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.extend(folded.to_lowercase());
    }
    out
}

/// Folds one character; `None` means "ignore it" (Arabic diacritics, tatweel).
fn fold_char(c: char) -> Option<char> {
    match c {
        '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0640}' => None,
        'أ' | 'إ' | 'آ' | 'ٱ' => Some('ا'),
        'ى' | 'ئ' => Some('ي'),
        'ؤ' => Some('و'),
        'ة' => Some('ه'),
        other => Some(other),
    }
}

/// The text stored per app for searching: normalised name and description.
pub fn search_text(name: &str, description: &str) -> String {
    let mut s = normalize(name);
    let d = normalize(description);
    if !d.is_empty() {
        s.push('\n');
        s.push_str(&d);
    }
    s
}

/// Builds a `LIKE` pattern (`ESCAPE '\'`) that matches `needle` anywhere, literally.
pub fn like_pattern(needle: &str) -> String {
    let mut p = String::with_capacity(needle.len() + 2);
    p.push('%');
    for c in needle.chars() {
        if matches!(c, '%' | '_' | '\\') {
            p.push('\\');
        }
        p.push(c);
    }
    p.push('%');
    p
}

/// Normalises a file type: leading dot removed, lower case. `None` if it is not 1–16 ASCII
/// letters or digits.
pub fn file_type(text: &str) -> Option<String> {
    let t = text.trim().trim_start_matches('.');
    let ok = (1..=16).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_alphanumeric());
    ok.then(|| t.to_ascii_lowercase())
}

/// True for identifiers the server accepts from clients and administrators (app ids, versions,
/// channels, architectures, editions, categories): 1–128 characters of `a-z A-Z 0-9 . _ - +`,
/// starting with a letter or digit and without `..`. Such a value can never form a path.
pub fn valid_ident(text: &str) -> bool {
    let bytes = text.as_bytes();
    let first_ok = bytes.first().is_some_and(|b| b.is_ascii_alphanumeric());
    first_ok
        && bytes.len() <= 128
        && !text.contains("..")
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
}

/// True for a lower-case hexadecimal SHA-256 digest (the only form of file name in the store).
pub fn valid_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_folds_arabic_and_case() {
        assert_eq!(normalize("  Hello   WORLD \n"), "hello world");
        assert_eq!(normalize("مَكْتَبَة"), "مكتبه");
        assert_eq!(normalize("أحمد إبراهيم آدم"), "احمد ابراهيم ادم");
        assert_eq!(normalize("مستشفى"), "مستشفي");
        assert_eq!(normalize("جـــدول"), "جدول");
        assert_eq!(normalize("ÄÖÜ"), "äöü");
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("\u{064B}\u{0640}"), "");
    }

    #[test]
    fn search_text_joins_name_and_description() {
        assert_eq!(
            search_text("KM Notes", "Write  notes"),
            "km notes\nwrite notes"
        );
        assert_eq!(search_text("KM Notes", ""), "km notes");
    }

    #[test]
    fn like_pattern_escapes_wildcards() {
        assert_eq!(like_pattern("a%b_c\\d"), "%a\\%b\\_c\\\\d%");
        assert_eq!(like_pattern(""), "%%");
    }

    #[test]
    fn file_type_rules() {
        assert_eq!(file_type(".XLSX").as_deref(), Some("xlsx"));
        assert_eq!(file_type("pdf").as_deref(), Some("pdf"));
        assert_eq!(file_type("."), None);
        assert_eq!(file_type("a/b"), None);
        assert_eq!(file_type("aaaaaaaaaaaaaaaaa"), None);
    }

    #[test]
    fn ident_rules_exclude_everything_path_like() {
        for good in ["com.kmcc.pos", "1.2.0", "0.7.0-b1+build5", "x86_64", "a"] {
            assert!(valid_ident(good), "{good}");
        }
        let long = "a".repeat(129);
        for bad in [
            "", ".", "..", "a..b", "../etc", "a/b", "a\\b", ".hidden", "a b", "a%2f", "é", &long,
        ] {
            assert!(!valid_ident(bad), "{bad}");
        }
    }

    #[test]
    fn sha256_rules() {
        assert!(valid_sha256(&"ab".repeat(32)));
        assert!(!valid_sha256(&"AB".repeat(32)));
        assert!(!valid_sha256(&"ab".repeat(31)));
        assert!(!valid_sha256(&format!("{}/.", "ab".repeat(31))));
    }
}
