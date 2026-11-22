//! Reader for policy files.
//!
//! Policy files are TOML. To keep the system core free of a full TOML
//! dependency this module reads exactly the subset a policy needs:
//! `[[rule]]` tables whose keys hold a string or an array of strings
//! (arrays may span lines), plus comments. Anything else is rejected with a
//! typed error instead of being guessed at. The test suite checks the reader
//! against a complete TOML implementation.

use crate::policy::{Policy, Rule};
use crate::topic::Pattern;
use std::fmt;

/// Why a policy file was rejected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum PolicyErrorKind {
    /// The file could not be read.
    Io(std::io::ErrorKind),
    /// A character that cannot start any token.
    UnexpectedCharacter,
    /// A string without closing quote on the same line.
    UnterminatedString,
    /// Valid TOML that is outside the supported subset (escapes, inline
    /// tables, numbers, …) or a token in the wrong place.
    UnsupportedSyntax,
    /// A table other than `[[rule]]`.
    UnknownTable,
    /// A key that no rule field corresponds to.
    UnknownKey,
    /// The same key twice in one rule.
    DuplicateKey,
    /// A key before the first `[[rule]]`.
    KeyOutsideRule,
    /// A rule without `identity`.
    MissingIdentity,
    /// `identity` must be a string, the other keys arrays of strings.
    WrongValueType,
    /// A string that is not a valid name or pattern.
    InvalidPattern,
}

/// A rejected policy file: what is wrong and on which line (1-based, `0`
/// when the file itself could not be read).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PolicyError {
    /// Line of the offending token.
    pub line: usize,
    /// What is wrong.
    pub kind: PolicyErrorKind,
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "policy line {}: {:?}", self.line, self.kind)
    }
}

impl std::error::Error for PolicyError {}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Token {
    Open,
    Close,
    Equals,
    Comma,
    Newline,
    Word(String),
    Text(String),
}

type Spanned = (usize, Token);

fn fail<T>(line: usize, kind: PolicyErrorKind) -> Result<T, PolicyError> {
    Err(PolicyError { line, kind })
}

fn read_string(line: usize, quote: char, rest: &str) -> Result<(String, usize), PolicyError> {
    let Some(end) = rest.find(quote) else {
        return fail(line, PolicyErrorKind::UnterminatedString);
    };
    let text = &rest[..end];
    if quote == '"' && text.contains('\\') {
        return fail(line, PolicyErrorKind::UnsupportedSyntax);
    }
    Ok((text.to_owned(), end + 1))
}

fn tokenize_line(line: usize, text: &str, out: &mut Vec<Spanned>) -> Result<(), PolicyError> {
    let mut rest = text;
    while let Some(first) = rest.chars().next() {
        let after = &rest[first.len_utf8()..];
        let simple = match first {
            '[' => Some(Token::Open),
            ']' => Some(Token::Close),
            '=' => Some(Token::Equals),
            ',' => Some(Token::Comma),
            _ => None,
        };
        if let Some(token) = simple {
            out.push((line, token));
            rest = after;
        } else if first == '#' {
            break;
        } else if first == ' ' || first == '\t' || first == '\r' {
            rest = after;
        } else if first == '"' || first == '\'' {
            let (value, used) = read_string(line, first, after)?;
            out.push((line, Token::Text(value)));
            rest = &after[used..];
        } else if first.is_ascii_alphanumeric() || first == '_' || first == '-' {
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                .unwrap_or(rest.len());
            out.push((line, Token::Word(rest[..end].to_owned())));
            rest = &rest[end..];
        } else {
            return fail(line, PolicyErrorKind::UnexpectedCharacter);
        }
    }
    out.push((line, Token::Newline));
    Ok(())
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Value {
    Single(String),
    List(Vec<String>),
}

struct Draft {
    line: usize,
    entries: Vec<(usize, String, Value)>,
}

struct Parser {
    tokens: Vec<Spanned>,
    position: usize,
    last_line: usize,
}

impl Parser {
    fn next(&mut self) -> Option<Token> {
        let (line, token) = self.tokens.get(self.position)?.clone();
        self.position += 1;
        self.last_line = line;
        Some(token)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position).map(|(_, token)| token)
    }

    fn require(&mut self, wanted: &Token) -> Result<(), PolicyError> {
        match self.next() {
            Some(token) if token == *wanted => Ok(()),
            _ => fail(self.last_line, PolicyErrorKind::UnsupportedSyntax),
        }
    }

    fn end_of_line(&mut self) -> Result<(), PolicyError> {
        match self.next() {
            None | Some(Token::Newline) => Ok(()),
            Some(_) => fail(self.last_line, PolicyErrorKind::UnsupportedSyntax),
        }
    }

    /// Reads the rest of a `[[rule]]` header; the first `[` is consumed.
    fn header(&mut self) -> Result<(), PolicyError> {
        self.require(&Token::Open)?;
        match self.next() {
            Some(Token::Word(name)) if name == "rule" => {}
            Some(Token::Word(_)) => return fail(self.last_line, PolicyErrorKind::UnknownTable),
            _ => return fail(self.last_line, PolicyErrorKind::UnsupportedSyntax),
        }
        self.require(&Token::Close)?;
        self.require(&Token::Close)?;
        self.end_of_line()
    }

    /// Reads the items of an array; the opening `[` is consumed.
    fn array(&mut self) -> Result<Vec<String>, PolicyError> {
        let mut items = Vec::new();
        let mut want_item = true;
        loop {
            match self.next() {
                Some(Token::Newline) => {}
                Some(Token::Close) => return Ok(items),
                Some(Token::Text(text)) if want_item => {
                    items.push(text);
                    want_item = false;
                }
                Some(Token::Comma) if !want_item => want_item = true,
                _ => return fail(self.last_line, PolicyErrorKind::UnsupportedSyntax),
            }
        }
    }

    fn value(&mut self) -> Result<Value, PolicyError> {
        match self.next() {
            Some(Token::Text(text)) => Ok(Value::Single(text)),
            Some(Token::Open) => self.array().map(Value::List),
            _ => fail(self.last_line, PolicyErrorKind::UnsupportedSyntax),
        }
    }

    fn drafts(&mut self) -> Result<Vec<Draft>, PolicyError> {
        let mut drafts: Vec<Draft> = Vec::new();
        while let Some(token) = self.next() {
            let line = self.last_line;
            match token {
                Token::Newline => {}
                Token::Open => {
                    if self.peek() != Some(&Token::Open) {
                        return fail(line, PolicyErrorKind::UnknownTable);
                    }
                    self.header()?;
                    drafts.push(Draft {
                        line,
                        entries: Vec::new(),
                    });
                }
                Token::Word(key) => {
                    self.require(&Token::Equals)?;
                    let value = self.value()?;
                    self.end_of_line()?;
                    let Some(draft) = drafts.last_mut() else {
                        return fail(line, PolicyErrorKind::KeyOutsideRule);
                    };
                    if draft
                        .entries
                        .iter()
                        .any(|(_, existing, _)| *existing == key)
                    {
                        return fail(line, PolicyErrorKind::DuplicateKey);
                    }
                    draft.entries.push((line, key, value));
                }
                _ => return fail(line, PolicyErrorKind::UnsupportedSyntax),
            }
        }
        Ok(drafts)
    }
}

fn pattern(line: usize, text: &str) -> Result<Pattern, PolicyError> {
    Pattern::parse(text).or_else(|_| fail(line, PolicyErrorKind::InvalidPattern))
}

fn build_rule(draft: Draft) -> Result<Rule, PolicyError> {
    let mut identity = None;
    let mut lists: [Vec<Pattern>; 4] = Default::default();
    for (line, key, value) in draft.entries {
        let slot = match key.as_str() {
            "identity" => None,
            "register" => Some(0),
            "call" => Some(1),
            "publish" => Some(2),
            "subscribe" => Some(3),
            _ => return fail(line, PolicyErrorKind::UnknownKey),
        };
        match (slot, value) {
            (None, Value::Single(text)) => identity = Some(pattern(line, &text)?),
            (Some(index), Value::List(items)) => {
                for item in &items {
                    lists[index].push(pattern(line, item)?);
                }
            }
            _ => return fail(line, PolicyErrorKind::WrongValueType),
        }
    }
    let Some(identity) = identity else {
        return fail(draft.line, PolicyErrorKind::MissingIdentity);
    };
    let [register, call, publish, subscribe] = lists;
    Ok(Rule {
        identity,
        register,
        call,
        publish,
        subscribe,
    })
}

/// Parses policy text; see the module documentation for the accepted subset.
pub(crate) fn parse_policy(text: &str) -> Result<Policy, PolicyError> {
    let mut tokens = Vec::new();
    for (index, line) in text.lines().enumerate() {
        tokenize_line(index + 1, line, &mut tokens)?;
    }
    let mut parser = Parser {
        tokens,
        position: 0,
        last_line: 1,
    };
    let rules = parser
        .drafts()?
        .into_iter()
        .map(build_rule)
        .collect::<Result<_, _>>()?;
    Ok(Policy { rules })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_of(text: &str) -> PolicyErrorKind {
        parse_policy(text).unwrap_err().kind
    }

    #[test]
    fn empty_and_comment_only_files_deny_everything() {
        assert_eq!(parse_policy("").unwrap(), Policy::deny_all());
        assert_eq!(
            parse_policy("# nothing\n\n   \n").unwrap(),
            Policy::deny_all()
        );
    }

    #[test]
    fn full_example_parses() {
        let text = r#"
# system core
[[rule]]
identity = "system.settingsd"   # trailing comment
register = ["settings"]
publish  = [
    "settings.changed",
    'settings.audit',   # literal string
]
subscribe = []

[[rule]]
identity = 'app.*'
call = ["settings.get", "notify.*"]
"#;
        let policy = parse_policy(text).unwrap();
        assert_eq!(policy.rules.len(), 2);
        assert_eq!(policy.rules[0].identity.to_string(), "system.settingsd");
        assert_eq!(policy.rules[0].publish.len(), 2);
        assert!(policy.rules[0].subscribe.is_empty());
        assert_eq!(policy.rules[1].call[1].to_string(), "notify.*");
    }

    fn expect_errors(cases: &[(&str, PolicyErrorKind, usize)]) {
        for (text, kind, line) in cases {
            let err = parse_policy(text).unwrap_err();
            assert_eq!((err.kind, err.line), (*kind, *line), "{text:?}");
        }
    }

    #[test]
    fn rule_content_errors_are_reported_with_their_line() {
        use PolicyErrorKind::*;
        expect_errors(&[
            ("identity = \"a\"\n", KeyOutsideRule, 1),
            ("[[rule]]\ncall = [\"*\"]\n", MissingIdentity, 1),
            (
                "[[rule]]\nidentity = \"a\"\nidentity = \"b\"",
                DuplicateKey,
                3,
            ),
            (
                "[[rule]]\nidentity = \"a\"\ncolour = [\"b\"]",
                UnknownKey,
                3,
            ),
            ("[[other]]\n", UnknownTable, 1),
            ("[rule]\n", UnknownTable, 1),
            ("[[rule]]\nidentity = [\"a\"]\n", WrongValueType, 2),
            (
                "[[rule]]\nidentity = \"a\"\ncall = \"x\"\n",
                WrongValueType,
                3,
            ),
            ("[[rule]]\nidentity = \"Not Valid\"\n", InvalidPattern, 2),
            (
                "[[rule]]\nidentity = \"a\"\ncall = [\"a..b\"]",
                InvalidPattern,
                3,
            ),
        ]);
    }

    #[test]
    fn syntax_errors_are_reported_with_their_line() {
        use PolicyErrorKind::*;
        expect_errors(&[
            ("[[rule]]\nidentity = \"a", UnterminatedString, 2),
            ("[[rule]]\nidentity = \"a\\n\"", UnsupportedSyntax, 2),
            ("[[rule]]\nidentity = 5\n", UnsupportedSyntax, 2),
            ("[[rule]]\nidentity = {}\n", UnexpectedCharacter, 2),
            ("[[rule]] identity = \"a\"\n", UnsupportedSyntax, 1),
            ("[[rule]]\nidentity = \"a\" \"b\"\n", UnsupportedSyntax, 2),
            ("[[rule]]\ncall = [\"a\" \"b\"]\n", UnsupportedSyntax, 2),
            ("[[rule]]\ncall = [,]\n", UnsupportedSyntax, 2),
            ("[[rule]]\ncall = [\"a\"\n", UnsupportedSyntax, 2),
            ("[[rule]]\n= \"a\"\n", UnsupportedSyntax, 2),
            ("[[rule\n", UnsupportedSyntax, 1),
        ]);
    }

    #[test]
    fn errors_display_line_and_kind() {
        let text = PolicyError {
            line: 4,
            kind: PolicyErrorKind::UnknownKey,
        }
        .to_string();
        assert!(text.contains('4') && text.contains("UnknownKey"));
        assert_eq!(kind_of("?"), PolicyErrorKind::UnexpectedCharacter);
    }

    #[test]
    fn arbitrary_bytes_never_panic() {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..2000 {
            let mut text = String::new();
            for _ in 0..40 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let alphabet = b"[]=,\"'#\n rule.identy*ca\\{}";
                text.push(alphabet[(state % alphabet.len() as u64) as usize] as char);
            }
            let _ = parse_policy(&text);
        }
    }
}
