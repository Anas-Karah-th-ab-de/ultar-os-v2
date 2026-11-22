//! A small Rust lexer: enough to tell code, comments and literals apart.
//!
//! It handles line and (nested) block comments, doc comments, string, byte
//! string, C string and raw string literals, char and byte literals, lifetimes
//! and raw identifiers. Everything else becomes single-character punctuation.

use super::{Token, TokenKind};

/// Splits Rust source text into tokens (comments included).
pub fn lex(source: &str) -> Vec<Token> {
    let mut lexer = Lexer {
        chars: source.chars().collect(),
        pos: 0,
        line: 1,
        out: Vec::new(),
    };
    while let Some(c) = lexer.peek(0) {
        lexer.step(c);
    }
    lexer.out
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: u32,
    out: Vec<Token>,
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

impl Lexer {
    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn push(&mut self, kind: TokenKind, line: u32, text: String) {
        self.out.push(Token {
            kind,
            line,
            end_line: self.line,
            text,
            in_test: false,
        });
    }

    fn step(&mut self, c: char) {
        let next = self.peek(1);
        if c.is_whitespace() {
            self.bump();
        } else if c == '/' && next == Some('/') {
            self.line_comment();
        } else if c == '/' && next == Some('*') {
            self.block_comment();
        } else if c == '"' {
            let line = self.line;
            self.bump();
            self.string(line);
        } else if c == '\'' {
            self.quote();
        } else if is_ident_start(c) {
            self.word();
        } else if c.is_ascii_digit() {
            self.number();
        } else {
            let line = self.line;
            self.bump();
            self.push(TokenKind::Punct, line, c.to_string());
        }
    }

    fn line_comment(&mut self) {
        let line = self.line;
        let mut text = String::new();
        while let Some(c) = self.peek(0) {
            if c == '\n' {
                break;
            }
            text.push(c);
            self.bump();
        }
        let doc = (text.starts_with("///") && !text.starts_with("////")) || text.starts_with("//!");
        let kind = if doc {
            TokenKind::DocComment
        } else {
            TokenKind::Comment
        };
        self.push(kind, line, text);
    }

    fn block_comment(&mut self) {
        let line = self.line;
        let mut text = String::new();
        let mut depth = 0u32;
        while let Some(c) = self.peek(0) {
            let next = self.peek(1);
            if c == '/' && next == Some('*') {
                depth += 1;
                text.push_str("/*");
                self.bump();
                self.bump();
            } else if c == '*' && next == Some('/') {
                depth = depth.saturating_sub(1);
                text.push_str("*/");
                self.bump();
                self.bump();
                if depth == 0 {
                    break;
                }
            } else {
                text.push(c);
                self.bump();
            }
        }
        let doc = (text.starts_with("/**") && !text.starts_with("/***") && text != "/**/")
            || text.starts_with("/*!");
        let kind = if doc {
            TokenKind::DocComment
        } else {
            TokenKind::Comment
        };
        self.push(kind, line, text);
    }

    /// Lexes a normal string body; the opening quote is already consumed.
    fn string(&mut self, line: u32) {
        let mut text = String::new();
        while let Some(c) = self.bump() {
            match c {
                '"' => break,
                '\\' => {
                    text.push(c);
                    if let Some(escaped) = self.bump() {
                        text.push(escaped);
                    }
                }
                _ => text.push(c),
            }
        }
        self.push(TokenKind::Str, line, text);
    }

    /// Lexes a raw string body; `r`, the hashes and the quote are consumed.
    fn raw_string(&mut self, line: u32, hashes: usize) {
        let mut text = String::new();
        while let Some(c) = self.bump() {
            if c == '"' && (0..hashes).all(|i| self.peek(i) == Some('#')) {
                for _ in 0..hashes {
                    self.bump();
                }
                break;
            }
            text.push(c);
        }
        self.push(TokenKind::Str, line, text);
    }

    /// Lexes a char literal or a lifetime starting at `'`.
    fn quote(&mut self) {
        let line = self.line;
        let first = self.peek(1);
        let second = self.peek(2);
        if first == Some('\\') {
            self.bump();
            self.bump();
            let mut text = String::from("\\");
            if let Some(escaped) = self.bump() {
                text.push(escaped);
            }
            while let Some(c) = self.bump() {
                if c == '\'' {
                    break;
                }
                text.push(c);
            }
            self.push(TokenKind::Char, line, text);
        } else if second == Some('\'') && first.is_some() && first != Some('\'') {
            self.bump();
            let text = self.bump().map(String::from).unwrap_or_default();
            self.bump();
            self.push(TokenKind::Char, line, text);
        } else {
            self.bump();
            let mut text = String::from("'");
            while let Some(c) = self.peek(0) {
                if !is_ident_continue(c) {
                    break;
                }
                text.push(c);
                self.bump();
            }
            self.push(TokenKind::Lifetime, line, text);
        }
    }

    fn take_ident(&mut self) -> String {
        let mut text = String::new();
        while let Some(c) = self.peek(0) {
            if !is_ident_continue(c) {
                break;
            }
            text.push(c);
            self.bump();
        }
        text
    }

    /// Lexes an identifier, or a literal introduced by a prefix (`r`, `b`, `c`, `br`, `cr`).
    fn word(&mut self) {
        let line = self.line;
        let ident = self.take_ident();
        let raw_prefix = matches!(ident.as_str(), "r" | "br" | "cr");
        let plain_prefix = matches!(ident.as_str(), "b" | "c");
        if raw_prefix {
            let mut hashes = 0;
            while self.peek(hashes) == Some('#') {
                hashes += 1;
            }
            if self.peek(hashes) == Some('"') {
                for _ in 0..=hashes {
                    self.bump();
                }
                self.raw_string(line, hashes);
                return;
            }
            if ident == "r" && hashes == 1 && self.peek(1).is_some_and(is_ident_start) {
                self.bump();
                let name = self.take_ident();
                self.push(TokenKind::Ident, line, name);
                return;
            }
        } else if plain_prefix && self.peek(0) == Some('"') {
            self.bump();
            self.string(line);
            return;
        } else if ident == "b" && self.peek(0) == Some('\'') {
            self.quote();
            return;
        }
        self.push(TokenKind::Ident, line, ident);
    }

    fn number(&mut self) {
        let line = self.line;
        let mut text = String::new();
        while let Some(c) = self.peek(0) {
            let fraction = c == '.' && self.peek(1).is_some_and(|n| n.is_ascii_digit());
            if !(is_ident_continue(c) || fraction) {
                break;
            }
            text.push(c);
            self.bump();
        }
        self.push(TokenKind::Number, line, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<(TokenKind, String)> {
        lex(source).into_iter().map(|t| (t.kind, t.text)).collect()
    }

    fn strings(source: &str) -> Vec<String> {
        lex(source)
            .into_iter()
            .filter(|t| t.kind == TokenKind::Str)
            .map(|t| t.text)
            .collect()
    }

    #[test]
    fn line_and_doc_comments_are_separated() {
        let tokens = kinds("// a \"/dev/x\"\n/// doc\n//! inner\n//// plain\nlet");
        assert_eq!(tokens[0].0, TokenKind::Comment);
        assert_eq!(tokens[1].0, TokenKind::DocComment);
        assert_eq!(tokens[2].0, TokenKind::DocComment);
        assert_eq!(tokens[3].0, TokenKind::Comment);
        assert_eq!(tokens[4], (TokenKind::Ident, "let".to_string()));
        assert!(strings("// \"/dev/x\"").is_empty());
    }

    #[test]
    fn nested_block_comments_end_at_the_outer_close() {
        let tokens = lex("/* a /* b \"/dev/\" */ c */ x /** doc */ /**/ /*! in */");
        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens[0].kind, TokenKind::Comment);
        assert_eq!(tokens[1].text, "x");
        assert_eq!(tokens[2].kind, TokenKind::DocComment);
        assert_eq!(tokens[3].kind, TokenKind::Comment);
        assert_eq!(tokens[4].kind, TokenKind::DocComment);
    }

    #[test]
    fn unterminated_block_comment_consumes_the_rest() {
        let tokens = lex("a /* never closed \"x\"");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[1].kind, TokenKind::Comment);
    }

    #[test]
    fn strings_keep_escapes_and_hide_comment_markers() {
        assert_eq!(strings(r#"let a = "x \" // y";"#), vec![r#"x \" // y"#]);
        assert_eq!(strings(r#""a /* b */ c""#), vec!["a /* b */ c"]);
        assert_eq!(strings(r#"b"bytes" c"cstr""#), vec!["bytes", "cstr"]);
        assert_eq!(strings(r#""end\\" x"#), vec![r"end\\"]);
    }

    #[test]
    fn raw_strings_respect_their_hashes() {
        assert_eq!(strings(r###"r"/dev/a""###), vec!["/dev/a"]);
        assert_eq!(strings(r###"r#"a "quoted" b"#"###), vec![r#"a "quoted" b"#]);
        assert_eq!(strings(r####"r##"a "# b"##"####), vec![r##"a "# b"##]);
        assert_eq!(strings(r###"br#"x"# cr"y""###), vec!["x", "y"]);
        assert_eq!(strings(r#"r"back\" x"#), vec![r"back\"]);
    }

    #[test]
    fn raw_identifiers_are_identifiers() {
        let tokens = kinds("r#type r #");
        assert_eq!(tokens[0], (TokenKind::Ident, "type".to_string()));
        assert_eq!(tokens[1], (TokenKind::Ident, "r".to_string()));
        assert_eq!(tokens[2], (TokenKind::Punct, "#".to_string()));
    }

    #[test]
    fn chars_and_lifetimes_are_told_apart() {
        let tokens = kinds(r#"'a' 'a 'static '\'' '\n' '"' b'x' b'\\' '\u{1F600}' 'ب'"#);
        let expected = [
            (TokenKind::Char, "a"),
            (TokenKind::Lifetime, "'a"),
            (TokenKind::Lifetime, "'static"),
            (TokenKind::Char, "\\'"),
            (TokenKind::Char, "\\n"),
            (TokenKind::Char, "\""),
            (TokenKind::Char, "x"),
            (TokenKind::Char, "\\\\"),
            (TokenKind::Char, "\\u{1F600}"),
            (TokenKind::Char, "ب"),
        ];
        assert_eq!(tokens.len(), expected.len());
        for (token, (kind, text)) in tokens.iter().zip(expected) {
            assert_eq!((token.0, token.1.as_str()), (kind, text));
        }
    }

    #[test]
    fn quote_char_does_not_open_a_string() {
        assert_eq!(strings(r#"let q = '"'; let s = "real";"#), vec!["real"]);
        assert_eq!(strings(r#"fn f<'a>(x: &'a str) { "s" }"#), vec!["s"]);
    }

    #[test]
    fn lines_are_tracked_through_multiline_tokens() {
        let tokens = lex("a\n\"x\ny\"\nb /* 1\n2 */ c");
        assert_eq!((tokens[0].line, tokens[0].end_line), (1, 1));
        assert_eq!((tokens[1].line, tokens[1].end_line), (2, 3));
        assert_eq!(tokens[2].line, 4);
        assert_eq!((tokens[3].line, tokens[3].end_line), (4, 5));
        assert_eq!(tokens[4].line, 5);
    }

    #[test]
    fn numbers_and_ranges() {
        let tokens = kinds("1.5 0x1f 1..2 3u8");
        assert_eq!(tokens[0], (TokenKind::Number, "1.5".to_string()));
        assert_eq!(tokens[1], (TokenKind::Number, "0x1f".to_string()));
        assert_eq!(tokens[2], (TokenKind::Number, "1".to_string()));
        assert_eq!(tokens[3].0, TokenKind::Punct);
        assert_eq!(tokens[5], (TokenKind::Number, "2".to_string()));
        assert_eq!(tokens[6], (TokenKind::Number, "3u8".to_string()));
    }

    #[test]
    fn unterminated_string_does_not_loop() {
        assert_eq!(strings("\"open"), vec!["open"]);
        assert_eq!(strings("r#\"open"), vec!["open"]);
        assert_eq!(lex("'").len(), 1);
    }
}
