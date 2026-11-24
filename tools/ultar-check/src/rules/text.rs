//! Rule `text`: no hardcoded user-visible text (plan §7 rule 7, §3.4b).
//!
//! Heuristic: a string literal that contains Arabic script or German
//! umlauts/sharp s is natural-language text and belongs into `i18n/*.ftl`.
//! Tests, comments, internal `msg` modules, the i18n crate and the brand source (names in
//! Arabic script) are exempt. The pieces of one `concat!(…)` are one text and count once.

use super::Rule;
use crate::finding::{excerpt, Finding, Message};
use crate::msg::Key;
use crate::scan::{SourceFile, Token, TokenKind};

/// Crates whose job is to hold or process translated text.
const EXEMPT_PREFIXES: [&str; 2] = ["system/core/ultar-i18n/", "system/core/ultar-brand/"];

/// Flags string literals with Arabic script or German special letters.
pub fn check(files: &[SourceFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files {
        if file.is_under(&EXEMPT_PREFIXES) || is_message_module(&file.path) {
            continue;
        }
        let mut joined_until = 0;
        for (index, token) in file.code.iter().enumerate() {
            let hit = token.kind == TokenKind::Str
                && !token.in_test
                && token.text.chars().any(is_natural_language_letter);
            if !hit {
                continue;
            }
            // later pieces of a `concat!(…)` that was already reported
            if index < joined_until {
                continue;
            }
            joined_until = concat_end(&file.code, index).unwrap_or(0);
            let message = Message::new(Key::HardcodedText, &[excerpt(&token.text)]);
            findings.push(Finding::error(Rule::Text, &file.path, token.line, message));
        }
    }
    findings
}

/// When the literal at `index` is a direct argument of `concat!(…)`: the index after the
/// closing parenthesis of that call.
fn concat_end(code: &[Token], index: usize) -> Option<usize> {
    let is = |token: &Token, text: &str| token.kind == TokenKind::Punct && token.text == text;
    // walk back to the opening parenthesis of the enclosing call
    let mut depth = 0usize;
    let mut open = None;
    for at in (0..index).rev() {
        let token = code.get(at)?;
        if is(token, ")") {
            depth += 1;
        } else if is(token, "(") {
            if depth == 0 {
                open = Some(at);
                break;
            }
            depth -= 1;
        }
    }
    let open = open?;
    let bang = code.get(open.checked_sub(1)?)?;
    let name = code.get(open.checked_sub(2)?)?;
    if !is(bang, "!") || name.kind != TokenKind::Ident || name.text != "concat" {
        return None;
    }
    let mut depth = 0usize;
    for (at, token) in code.iter().enumerate().skip(open) {
        if is(token, "(") {
            depth += 1;
        } else if is(token, ")") {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(at + 1);
            }
        }
    }
    None
}

/// Whether the file is an internal message table (`msg.rs` or below `msg/`).
pub fn is_message_module(path: &str) -> bool {
    let mut parts: Vec<&str> = path.split('/').collect();
    let file = parts.pop().unwrap_or("");
    file == "msg.rs" || parts.contains(&"msg")
}

fn is_natural_language_letter(c: char) -> bool {
    is_arabic(c) || is_german_special(c)
}

/// Arabic, Arabic Supplement, Extended-A and both presentation-form blocks.
pub fn is_arabic(c: char) -> bool {
    matches!(u32::from(c),
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
}

fn is_german_special(c: char) -> bool {
    matches!(
        u32::from(c),
        0xE4 | 0xF6 | 0xFC | 0xC4 | 0xD6 | 0xDC | 0xDF | 0x1E9E
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letter_classes() {
        assert!(is_arabic('ب'));
        assert!(is_arabic('\u{FE8E}'));
        assert!(!is_arabic('a'));
        for c in ['ä', 'ö', 'ü', 'Ä', 'Ö', 'Ü', 'ß'] {
            assert!(is_german_special(c));
        }
        assert!(!is_german_special('é'));
    }

    #[test]
    fn pieces_of_one_concat_are_one_text() {
        let source = "const A: &str = concat!(\"مرحباً بك في \", name!(), \"؟\");\n\
                      const B: &str = concat!(\"plain \", name!(), \" größer\", \"ß\");\n\
                      fn f() { g(\"أ\", \"ب\"); h(concat!(\"x\", \"y\"), \"ج\"); }\n";
        let file = SourceFile::parse("apps/a/src/lib.rs", source);
        let lines: Vec<u32> = check(&[file]).iter().map(|f| f.line).collect();
        // one per concat, but ordinary call arguments stay separate texts
        assert_eq!(lines, [1, 2, 3, 3, 3]);
    }

    #[test]
    fn the_brand_source_may_hold_names_in_arabic_script() {
        let source = "pub const D: &str = \"محمد\";\n";
        let brand = SourceFile::parse("system/core/ultar-brand/src/lib.rs", source);
        let other = SourceFile::parse("system/core/ultar-other/src/lib.rs", source);
        assert!(check(&[brand]).is_empty());
        assert_eq!(check(&[other]).len(), 1);
    }

    #[test]
    fn message_modules_are_recognised() {
        assert!(is_message_module("tools/x/src/msg.rs"));
        assert!(is_message_module("apps/a/src/msg/ar.rs"));
        assert!(!is_message_module("apps/a/src/msgs.rs"));
    }
}
