//! Rust-aware source scanning: tokens, test regions and small query helpers.

pub mod lexer;
pub mod test_regions;

/// Kind of a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// Identifier or keyword.
    Ident,
    /// Lifetime or loop label, including the leading `'`.
    Lifetime,
    /// String literal of any flavour; `text` is the raw content without quotes.
    Str,
    /// Char or byte literal; `text` is the raw content without quotes.
    Char,
    /// Numeric literal.
    Number,
    /// One punctuation character.
    Punct,
    /// Ordinary comment (line or block), markers included.
    Comment,
    /// Doc comment (`///`, `//!`, `/** */`, `/*! */`), markers included.
    DocComment,
}

/// One token of a Rust source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Kind of the token.
    pub kind: TokenKind,
    /// 1-based line where the token starts.
    pub line: u32,
    /// 1-based line where the token ends.
    pub end_line: u32,
    /// Token text (see [`TokenKind`]).
    pub text: String,
    /// Whether the token belongs to test code.
    pub in_test: bool,
}

/// A lexed Rust source file.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Repository-relative path with `/` separators.
    pub path: String,
    /// Code tokens (no comments), with test regions marked.
    pub code: Vec<Token>,
    /// Comment tokens (ordinary and doc comments).
    pub comments: Vec<Token>,
    /// Number of lines in the file.
    pub line_count: u32,
}

impl SourceFile {
    /// Lexes `text` and marks test code.
    pub fn parse(path: &str, text: &str) -> SourceFile {
        let (comments, mut code): (Vec<Token>, Vec<Token>) = lexer::lex(text)
            .into_iter()
            .partition(|t| matches!(t.kind, TokenKind::Comment | TokenKind::DocComment));
        if is_test_path(path) {
            for token in &mut code {
                token.in_test = true;
            }
        } else {
            test_regions::mark_tests(&mut code);
        }
        let line_count = u32::try_from(text.lines().count()).unwrap_or(u32::MAX);
        SourceFile {
            path: path.to_string(),
            code,
            comments,
            line_count,
        }
    }

    /// Whether the file lies below one of the given path prefixes.
    pub fn is_under(&self, prefixes: &[&str]) -> bool {
        prefixes.iter().any(|p| self.path.starts_with(p))
    }
}

/// Whether a path is test code by convention (`tests/`, `benches/`, `tests.rs`, `*_test(s).rs`).
pub fn is_test_path(path: &str) -> bool {
    let mut parts: Vec<&str> = path.split('/').collect();
    let file = parts.pop().unwrap_or("");
    parts.iter().any(|p| *p == "tests" || *p == "benches")
        || file == "tests.rs"
        || file.ends_with("_test.rs")
        || file.ends_with("_tests.rs")
}

/// Whether token `i` is the identifier `name`.
pub fn ident_at(tokens: &[Token], i: usize, name: &str) -> bool {
    tokens
        .get(i)
        .is_some_and(|t| t.kind == TokenKind::Ident && t.text == name)
}

/// Whether token `i` is any identifier.
pub fn any_ident_at(tokens: &[Token], i: usize) -> bool {
    tokens.get(i).is_some_and(|t| t.kind == TokenKind::Ident)
}

/// Whether token `i` is the punctuation character `ch`.
pub fn punct_at(tokens: &[Token], i: usize, ch: char) -> bool {
    tokens
        .get(i)
        .is_some_and(|t| t.kind == TokenKind::Punct && t.text.starts_with(ch))
}

/// Whether tokens `i` and `i + 1` form the path separator `::`.
pub fn path_sep_at(tokens: &[Token], i: usize) -> bool {
    punct_at(tokens, i, ':') && punct_at(tokens, i + 1, ':')
}

/// Finds the bracket closing the one opened at `open` (`(`, `[` or `{`).
pub fn matching_close(tokens: &[Token], open: usize) -> Option<usize> {
    let open_ch = tokens.get(open)?.text.chars().next()?;
    let close_ch = match open_ch {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => return None,
    };
    let mut depth = 0usize;
    for (offset, token) in tokens.get(open..)?.iter().enumerate() {
        if token.kind != TokenKind::Punct {
            continue;
        }
        let ch = token.text.chars().next();
        if ch == Some(open_ch) {
            depth += 1;
        } else if ch == Some(close_ch) {
            depth -= 1;
            if depth == 0 {
                return Some(open + offset);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_splits_comments_from_code() {
        let file = SourceFile::parse("a/src/lib.rs", "// c\nfn f() {} /* d */\n/// doc\n");
        assert_eq!(file.comments.len(), 3);
        assert_eq!(file.code.len(), 6);
        assert_eq!(file.line_count, 3);
        assert!(file.code.iter().all(|t| !t.in_test));
    }

    #[test]
    fn test_paths_mark_everything() {
        for path in [
            "a/tests/it.rs",
            "a/src/tests.rs",
            "a/benches/b.rs",
            "a/src/x_test.rs",
            "a/src/x_tests.rs",
            "tests/common/mod.rs",
        ] {
            assert!(is_test_path(path), "{path}");
            let file = SourceFile::parse(path, "fn f() {}");
            assert!(file.code.iter().all(|t| t.in_test));
        }
        assert!(!is_test_path("a/src/contests.rs"));
        assert!(!is_test_path("a/src/testing/mod.rs"));
    }

    #[test]
    fn is_under_matches_prefixes() {
        let file = SourceFile::parse("system/hal/x/src/lib.rs", "");
        assert!(file.is_under(&["drivers/", "system/hal/"]));
        assert!(!file.is_under(&["system/core/"]));
    }

    #[test]
    fn query_helpers_work_on_token_slices() {
        let file = SourceFile::parse("a.rs", "a::b(c[1], {2})");
        let t = &file.code;
        assert!(ident_at(t, 0, "a"));
        assert!(!ident_at(t, 0, "b"));
        assert!(any_ident_at(t, 3));
        assert!(!any_ident_at(t, 1));
        assert!(path_sep_at(t, 1));
        assert!(!path_sep_at(t, 2));
        assert!(punct_at(t, 4, '('));
        assert_eq!(matching_close(t, 4), Some(t.len() - 1));
        assert_eq!(matching_close(t, 6), Some(8));
        assert_eq!(matching_close(t, 0), None);
        assert_eq!(matching_close(t, 99), None);
    }

    #[test]
    fn matching_close_reports_unbalanced_input() {
        let file = SourceFile::parse("a.rs", "{ { }");
        assert_eq!(matching_close(&file.code, 0), None);
        assert_eq!(matching_close(&file.code, 1), Some(2));
    }
}
