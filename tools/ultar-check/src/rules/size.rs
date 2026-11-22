//! Rule `size`: file, function and struct size limits (plan §7 rule 4).

use super::Rule;
use crate::finding::{Finding, Message};
use crate::msg::Key;
use crate::scan::{any_ident_at, ident_at, matching_close, punct_at, SourceFile, Token, TokenKind};

/// A file longer than this fails.
pub const MAX_FILE_LINES: u32 = 600;
/// A file longer than this produces a warning.
pub const WARN_FILE_LINES: u32 = 400;
/// A function longer than this (signature to closing brace) fails.
pub const MAX_FN_LINES: u32 = 60;
/// A struct with more fields than this fails.
pub const MAX_STRUCT_FIELDS: usize = 15;

/// Checks all size limits.
pub fn check(files: &[SourceFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files {
        check_file_length(file, &mut findings);
        check_functions(file, &mut findings);
        check_structs(file, &mut findings);
    }
    findings
}

fn check_file_length(file: &SourceFile, findings: &mut Vec<Finding>) {
    let lines = file.line_count.to_string();
    if file.line_count > MAX_FILE_LINES {
        let message = Message::new(Key::FileTooLong, &[lines, MAX_FILE_LINES.to_string()]);
        findings.push(Finding::error(Rule::Size, &file.path, 1, message));
    } else if file.line_count > WARN_FILE_LINES {
        let message = Message::new(Key::FileLong, &[lines, WARN_FILE_LINES.to_string()]);
        findings.push(Finding::warning(Rule::Size, &file.path, 1, message));
    }
}

fn check_functions(file: &SourceFile, findings: &mut Vec<Finding>) {
    let tokens = &file.code;
    for i in 0..tokens.len() {
        if !(ident_at(tokens, i, "fn") && any_ident_at(tokens, i + 1)) {
            continue;
        }
        let Some(close) = body_open(tokens, i + 2).and_then(|open| matching_close(tokens, open))
        else {
            continue;
        };
        let length = tokens[close].line - tokens[i].line + 1;
        if length > MAX_FN_LINES {
            let args = [
                tokens[i + 1].text.clone(),
                length.to_string(),
                MAX_FN_LINES.to_string(),
            ];
            let message = Message::new(Key::FnTooLong, &args);
            findings.push(Finding::error(
                Rule::Size,
                &file.path,
                tokens[i].line,
                message,
            ));
        }
    }
}

/// Index of the `{` opening a function body, or `None` for a declaration.
fn body_open(tokens: &[Token], from: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(from) {
        if token.kind != TokenKind::Punct {
            continue;
        }
        match token.text.as_str() {
            "(" | "[" => depth += 1,
            ")" | "]" => depth = depth.saturating_sub(1),
            ";" if depth == 0 => return None,
            "{" if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn check_structs(file: &SourceFile, findings: &mut Vec<Finding>) {
    let tokens = &file.code;
    for i in 0..tokens.len() {
        if !(ident_at(tokens, i, "struct") && any_ident_at(tokens, i + 1)) {
            continue;
        }
        let fields = struct_fields(tokens, i + 2);
        if fields > MAX_STRUCT_FIELDS {
            let args = [
                tokens[i + 1].text.clone(),
                fields.to_string(),
                MAX_STRUCT_FIELDS.to_string(),
            ];
            let message = Message::new(Key::StructTooLarge, &args);
            findings.push(Finding::error(
                Rule::Size,
                &file.path,
                tokens[i].line,
                message,
            ));
        }
    }
}

/// Number of fields of the struct whose generics/body start at `from`.
fn struct_fields(tokens: &[Token], from: usize) -> usize {
    let mut angle = 0usize;
    let mut in_where = false;
    for i in from..tokens.len() {
        if ident_at(tokens, i, "where") {
            in_where = true;
        }
        if tokens[i].kind != TokenKind::Punct {
            continue;
        }
        match tokens[i].text.as_str() {
            "<" => angle += 1,
            ">" if !punct_at(tokens, i.wrapping_sub(1), '-') => angle = angle.saturating_sub(1),
            ";" if angle == 0 => return 0,
            "{" if angle == 0 => {
                let close = matching_close(tokens, i).unwrap_or(tokens.len() - 1);
                return named_fields(&tokens[i..=close]);
            }
            "(" if angle == 0 && !in_where => {
                let close = matching_close(tokens, i).unwrap_or(tokens.len() - 1);
                return tuple_fields(&tokens[i..=close]);
            }
            _ => {}
        }
    }
    0
}

/// Counts `name:` entries directly inside the braces of a struct body.
fn named_fields(body: &[Token]) -> usize {
    let mut depth = 0usize;
    let mut count = 0;
    for (i, token) in body.iter().enumerate() {
        if token.kind == TokenKind::Punct {
            match token.text.as_str() {
                "{" | "(" | "[" => depth += 1,
                "}" | ")" | "]" => depth = depth.saturating_sub(1),
                _ => {}
            }
            continue;
        }
        let is_name = depth == 1
            && token.kind == TokenKind::Ident
            && punct_at(body, i + 1, ':')
            && !punct_at(body, i + 2, ':')
            && i > 0
            && starts_field(&body[i - 1]);
        if is_name {
            count += 1;
        }
    }
    count
}

fn starts_field(previous: &Token) -> bool {
    match previous.kind {
        TokenKind::Punct => matches!(previous.text.as_str(), "{" | "," | "]" | ")"),
        TokenKind::Ident => previous.text == "pub",
        _ => false,
    }
}

/// Counts the comma-separated entries directly inside the parentheses of a tuple struct.
fn tuple_fields(body: &[Token]) -> usize {
    let mut depth = 0usize;
    let mut angle = 0usize;
    let mut count = 0;
    let mut entry_has_tokens = false;
    for (i, token) in body.iter().enumerate() {
        let text = if token.kind == TokenKind::Punct {
            token.text.as_str()
        } else {
            ""
        };
        match text {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            "<" => angle += 1,
            ">" if !punct_at(body, i.wrapping_sub(1), '-') => angle = angle.saturating_sub(1),
            _ => {}
        }
        let separator = (text == "," && depth == 1 && angle == 0) || (text == ")" && depth == 0);
        if separator {
            count += usize::from(entry_has_tokens);
            entry_has_tokens = false;
        } else if !(text == "(" && depth == 1) {
            entry_has_tokens = true;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(source: &str) -> usize {
        let file = SourceFile::parse("a.rs", source);
        struct_fields(&file.code, 2)
    }

    #[test]
    fn named_fields_ignore_nested_types_and_attributes() {
        assert_eq!(
            fields("struct A { a: u8, pub b: Vec<(u8, u16)>, pub(crate) c: [u8; 4] }"),
            3
        );
        assert_eq!(
            fields("struct A { #[serde(default)] a: std::vec::Vec<u8>, }"),
            1
        );
        assert_eq!(
            fields("struct A { a: HashMap<K, V>, b: fn(x: u8) -> u8 }"),
            2
        );
        assert_eq!(fields("struct A {}"), 0);
        assert_eq!(fields("struct A;"), 0);
    }

    #[test]
    fn generics_and_where_clauses_are_skipped() {
        assert_eq!(
            fields("struct A<T: Into<u8>, const N: usize> { a: T, b: [u8; N] }"),
            2
        );
        assert_eq!(fields("struct A<F> where F: Fn(u8) -> u8 { f: F }"), 1);
    }

    #[test]
    fn tuple_fields_are_counted() {
        assert_eq!(fields("struct A(u8, Vec<(u8, u8)>, HashMap<K, V>);"), 3);
        assert_eq!(fields("struct A(pub u8,);"), 1);
        assert_eq!(fields("struct A();"), 0);
        assert_eq!(fields("struct A<T>(T, fn(u8) -> u8) where T: Copy;"), 2);
    }

    #[test]
    fn declarations_without_body_have_no_length() {
        let file = SourceFile::parse("a.rs", "trait T { fn a(&self, x: [u8; 2]); fn b() {} }");
        assert_eq!(body_open(&file.code, 5), None);
        let b = file.code.iter().position(|t| t.text == "b").unwrap();
        assert!(body_open(&file.code, b + 1).is_some());
    }
}
