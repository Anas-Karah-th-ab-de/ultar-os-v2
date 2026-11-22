//! Rule `text`: no hardcoded user-visible text (plan §7 rule 7, §3.4b).
//!
//! Heuristic: a string literal that contains Arabic script or German
//! umlauts/sharp s is natural-language text and belongs into `i18n/*.ftl`.
//! Tests, comments, internal `msg` modules and the i18n crate are exempt.

use super::Rule;
use crate::finding::{excerpt, Finding, Message};
use crate::msg::Key;
use crate::scan::{SourceFile, TokenKind};

/// Crates whose job is to hold or process translated text.
const EXEMPT_PREFIXES: [&str; 1] = ["system/core/ultar-i18n/"];

/// Flags string literals with Arabic script or German special letters.
pub fn check(files: &[SourceFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files {
        if file.is_under(&EXEMPT_PREFIXES) || is_message_module(&file.path) {
            continue;
        }
        for token in &file.code {
            let hit = token.kind == TokenKind::Str
                && !token.in_test
                && token.text.chars().any(is_natural_language_letter);
            if hit {
                let message = Message::new(Key::HardcodedText, &[excerpt(&token.text)]);
                findings.push(Finding::error(Rule::Text, &file.path, token.line, message));
            }
        }
    }
    findings
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
    fn message_modules_are_recognised() {
        assert!(is_message_module("tools/x/src/msg.rs"));
        assert!(is_message_module("apps/a/src/msg/ar.rs"));
        assert!(!is_message_module("apps/a/src/msgs.rs"));
    }
}
