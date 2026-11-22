//! Rule `unwrap`: no panicking shortcuts outside test code (plan §7 rule 5).

use super::Rule;
use crate::finding::{Finding, Message};
use crate::msg::Key;
use crate::scan::{punct_at, SourceFile, Token, TokenKind};

/// Methods that panic on the error case.
const PANICKING_METHODS: [&str; 2] = ["unwrap", "expect"];

/// Macros that panic unconditionally.
const PANICKING_MACROS: [&str; 3] = ["panic", "todo", "unimplemented"];

/// Flags `.unwrap()`, `.expect(…)`, `panic!`, `todo!` and `unimplemented!`.
pub fn check(files: &[SourceFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files {
        for (i, token) in file.code.iter().enumerate() {
            if token.in_test || token.kind != TokenKind::Ident {
                continue;
            }
            if let Some(shown) = panicking_call(&file.code, i) {
                let message = Message::new(Key::PanicCall, &[shown]);
                findings.push(Finding::error(
                    Rule::Unwrap,
                    &file.path,
                    token.line,
                    message,
                ));
            }
        }
    }
    findings
}

/// Display form of the panicking call at identifier `i`, if it is one.
fn panicking_call(tokens: &[Token], i: usize) -> Option<String> {
    let name = tokens.get(i)?.text.as_str();
    let is_method = i > 0
        && PANICKING_METHODS.contains(&name)
        && punct_at(tokens, i - 1, '.')
        && punct_at(tokens, i + 1, '(');
    if is_method {
        let empty_call = punct_at(tokens, i + 2, ')');
        let tail = if empty_call { "()" } else { "(" };
        return Some(format!(".{name}{tail}"));
    }
    let is_macro = PANICKING_MACROS.contains(&name) && punct_at(tokens, i + 1, '!');
    is_macro.then(|| format!("{name}!"))
}
