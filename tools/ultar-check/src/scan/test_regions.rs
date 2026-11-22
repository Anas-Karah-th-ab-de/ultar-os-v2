//! Detection of test code: items behind `#[cfg(test)]`, `#[test]` and friends.

use super::{matching_close, punct_at, Token, TokenKind};

/// Marks every token that belongs to a test item (`in_test = true`).
///
/// An inner `#![cfg(test)]` marks the whole file.
pub fn mark_tests(tokens: &mut [Token]) {
    let mut i = 0;
    while i < tokens.len() {
        let Some(attr) = attribute_at(tokens, i) else {
            i += 1;
            continue;
        };
        if !is_test_attribute(&tokens[i..=attr.end]) {
            i = attr.end + 1;
            continue;
        }
        if attr.inner {
            mark(tokens, 0, tokens.len().saturating_sub(1));
            return;
        }
        let end = item_end(tokens, attr.end + 1);
        mark(tokens, i, end);
        i = end + 1;
    }
}

struct Attribute {
    inner: bool,
    end: usize,
}

fn attribute_at(tokens: &[Token], i: usize) -> Option<Attribute> {
    if !punct_at(tokens, i, '#') {
        return None;
    }
    let inner = punct_at(tokens, i + 1, '!');
    let open = if inner { i + 2 } else { i + 1 };
    if !punct_at(tokens, open, '[') {
        return None;
    }
    let end = matching_close(tokens, open)?;
    Some(Attribute { inner, end })
}

fn is_test_attribute(attr: &[Token]) -> bool {
    let idents: Vec<&str> = attr
        .iter()
        .filter(|t| t.kind == TokenKind::Ident)
        .map(|t| t.text.as_str())
        .collect();
    match idents.first() {
        Some(&"cfg") => idents.contains(&"test") && !idents.contains(&"not"),
        Some(&"cfg_attr") | None => false,
        Some(_) => attribute_path_end(attr) == Some("test"),
    }
}

/// Last identifier of the attribute path (`tokio::test` -> `test`).
fn attribute_path_end(attr: &[Token]) -> Option<&str> {
    let mut last = None;
    for token in attr {
        match token.kind {
            TokenKind::Ident => last = Some(token.text.as_str()),
            TokenKind::Punct if token.text == "(" || token.text == "=" => break,
            _ => {}
        }
    }
    last
}

/// Index of the last token of the item starting at `start`.
fn item_end(tokens: &[Token], start: usize) -> usize {
    let last = tokens.len().saturating_sub(1);
    let mut i = start;
    while let Some(attr) = attribute_at(tokens, i) {
        i = attr.end + 1;
    }
    let mut depth = 0usize;
    while i < tokens.len() {
        if tokens[i].kind == TokenKind::Punct {
            match tokens[i].text.as_str() {
                "(" | "[" => depth += 1,
                ")" | "]" => depth = depth.saturating_sub(1),
                ";" if depth == 0 => return i,
                "{" => return matching_close(tokens, i).unwrap_or(last),
                _ => {}
            }
        }
        i += 1;
    }
    last
}

fn mark(tokens: &mut [Token], from: usize, to: usize) {
    for token in tokens.iter_mut().take(to + 1).skip(from) {
        token.in_test = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::lexer::lex;

    fn marked(source: &str) -> Vec<(String, bool)> {
        let mut tokens = lex(source);
        mark_tests(&mut tokens);
        tokens
            .into_iter()
            .filter(|t| t.kind == TokenKind::Ident)
            .map(|t| (t.text, t.in_test))
            .collect()
    }

    fn is_test(source: &str, ident: &str) -> bool {
        marked(source)
            .into_iter()
            .find(|(name, _)| name == ident)
            .map(|(_, flag)| flag)
            .unwrap()
    }

    #[test]
    fn cfg_test_module_is_marked_and_code_after_is_not() {
        let source = "fn live() {} #[cfg(test)] mod tests { fn inner() { x } } fn after() {}";
        assert!(!is_test(source, "live"));
        assert!(is_test(source, "inner"));
        assert!(is_test(source, "x"));
        assert!(!is_test(source, "after"));
    }

    #[test]
    fn test_functions_with_extra_attributes_are_marked() {
        let source = "#[test] #[ignore] fn t() { body } fn live() {}";
        assert!(is_test(source, "body"));
        assert!(!is_test(source, "live"));
        let source = "#[tokio::test(flavor = \"x\")] async fn t() { body } fn live() {}";
        assert!(is_test(source, "body"));
        assert!(!is_test(source, "live"));
    }

    #[test]
    fn cfg_variants() {
        assert!(is_test("#[cfg(all(test, unix))] fn a() { x }", "x"));
        assert!(!is_test("#[cfg(not(test))] fn a() { x }", "x"));
        assert!(!is_test(
            "#[cfg_attr(test, derive(Debug))] struct S { x: u8 }",
            "x"
        ));
        assert!(!is_test("#[cfg(feature = \"test\")] fn a() { x }", "x"));
        assert!(!is_test("#[derive(Debug)] struct S { x: u8 }", "x"));
    }

    #[test]
    fn items_ending_in_semicolon_stop_there() {
        let source = "#[cfg(test)] use a::b; fn live() {}";
        assert!(is_test(source, "b"));
        assert!(!is_test(source, "live"));
        let source = "#[cfg(test)] const N: [u8; 2] = [1, 2]; fn live() {}";
        assert!(!is_test(source, "live"));
        let source = "#[cfg(test)] mod tests; fn live() {}";
        assert!(!is_test(source, "live"));
    }

    #[test]
    fn inner_attribute_marks_the_whole_file() {
        let source = "fn before() {} #![cfg(test)] fn after() {}";
        assert!(is_test(source, "before"));
        assert!(is_test(source, "after"));
    }

    #[test]
    fn unbalanced_input_marks_to_the_end() {
        assert!(is_test("#[test] fn t() { open", "open"));
        assert!(marked("#[").iter().all(|(_, flag)| !flag));
    }
}
