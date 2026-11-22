//! Parser for the Fluent-like catalog format.
//!
//! ```text
//! # comment
//! key = text with {placeholder}
//! other.key = first line
//!     continuation line
//! count.files = {count ->
//!     [one] one file
//!    *[other] {count} files
//! }
//! brace = a literal {"{"} brace
//! ```

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::error::{ParseError, ParseErrorKind};
use crate::pattern::{Part, Select, Variant, VariantKey};

/// Maximum nesting depth of selections.
const MAX_DEPTH: usize = 4;

/// One `key = value` entry with continuation lines joined.
#[derive(Debug)]
pub(crate) struct RawEntry {
    pub(crate) line: usize,
    pub(crate) key: String,
    pub(crate) value: String,
}

/// `true` if `key` is a valid message key or namespace.
pub(crate) fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    let starts_well = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    starts_well
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        && !key.ends_with('.')
        && !key.contains("..")
}

/// Brace depth at the end of `value`, ignoring braces inside `{"…"}` literals.
fn open_braces(value: &str) -> usize {
    let mut depth = 0usize;
    let mut in_literal = false;
    let mut escaped = false;
    for c in value.chars() {
        if in_literal {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_literal = false,
                _ => {}
            }
        } else {
            match c {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                '"' if depth > 0 => in_literal = true,
                _ => {}
            }
        }
    }
    depth
}

/// Splits a source into entries, joining continuation lines with `\n`.
pub(crate) fn split_entries(source: &str) -> Result<Vec<RawEntry>, ParseError> {
    let mut entries: Vec<RawEntry> = Vec::new();
    let mut open = false;
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    for (index, raw) in source.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim();
        let inside = open && entries.last().is_some_and(|e| open_braces(&e.value) > 0);
        if inside {
            if let Some(entry) = entries.last_mut() {
                if !trimmed.is_empty() {
                    entry.value.push('\n');
                    entry.value.push_str(trimmed);
                }
            }
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            open = false;
            continue;
        }
        if raw.starts_with([' ', '\t']) {
            match entries.last_mut() {
                Some(entry) if open => {
                    entry.value.push('\n');
                    entry.value.push_str(trimmed);
                }
                _ => {
                    return Err(ParseError::new(
                        line,
                        ParseErrorKind::OrphanContinuation,
                        None,
                    ))
                }
            }
            continue;
        }
        entries.push(start_entry(line, trimmed)?);
        open = true;
    }
    Ok(entries)
}

fn start_entry(line: usize, text: &str) -> Result<RawEntry, ParseError> {
    let Some((key, value)) = text.split_once('=') else {
        return Err(ParseError::new(line, ParseErrorKind::MissingEquals, None));
    };
    let key = key.trim();
    if !is_valid_key(key) {
        return Err(ParseError::new(
            line,
            ParseErrorKind::InvalidKey,
            Some(key.to_string()),
        ));
    }
    Ok(RawEntry {
        line,
        key: key.to_string(),
        value: value.trim().to_string(),
    })
}

/// Parses the text of one entry into message parts.
pub(crate) fn parse_value(value: &str) -> Result<Vec<Part>, ParseErrorKind> {
    if value.trim().is_empty() {
        return Err(ParseErrorKind::EmptyValue);
    }
    let mut cursor = Cursor {
        source: value,
        position: 0,
    };
    cursor.pattern(0, false)
}

struct Cursor<'a> {
    source: &'a str,
    position: usize,
}

fn push_text(parts: &mut Vec<Part>, text: &str) {
    if text.is_empty() {
        return;
    }
    match parts.last_mut() {
        Some(Part::Text(existing)) => existing.push_str(text),
        _ => parts.push(Part::Text(text.to_string())),
    }
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-')
}

impl Cursor<'_> {
    fn rest(&self) -> &str {
        &self.source[self.position..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.position += c.len_utf8();
        Some(c)
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
    }

    fn after_whitespace(&self) -> bool {
        self.source[..self.position]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace)
    }

    /// Length and key of a variant marker (`[key]` or `*[key]`) at the current position.
    fn marker(&self) -> Option<(usize, bool, &str)> {
        let rest = self.rest();
        let (is_default, body) = match rest.strip_prefix('*') {
            Some(body) => (true, body),
            None => (false, rest),
        };
        let body = body.strip_prefix('[')?;
        let end = body.find(']')?;
        let key = &body[..end];
        let valid = !key.is_empty() && key.chars().all(is_name_char);
        let length = usize::from(is_default) + 1 + end + 1;
        valid.then_some((length, is_default, key))
    }

    fn pattern(&mut self, depth: usize, in_variant: bool) -> Result<Vec<Part>, ParseErrorKind> {
        let mut parts = Vec::new();
        let mut start = self.position;
        loop {
            let at_marker = in_variant && self.after_whitespace() && self.marker().is_some();
            match self.peek() {
                None => break,
                Some('}') if in_variant => break,
                Some('}') => return Err(ParseErrorKind::UnexpectedClosingBrace),
                Some(_) if at_marker => break,
                Some('{') => {
                    push_text(&mut parts, &self.source[start..self.position]);
                    self.bump();
                    self.braced(depth, &mut parts)?;
                    start = self.position;
                }
                Some(_) => {
                    self.bump();
                }
            }
        }
        push_text(&mut parts, &self.source[start..self.position]);
        Ok(parts)
    }

    fn braced(&mut self, depth: usize, parts: &mut Vec<Part>) -> Result<(), ParseErrorKind> {
        self.skip_whitespace();
        if self.peek() == Some('"') {
            self.bump();
            let literal = self.literal()?;
            self.skip_whitespace();
            return match self.bump() {
                Some('}') => {
                    push_text(parts, &literal);
                    Ok(())
                }
                Some(_) => Err(ParseErrorKind::InvalidPlaceholder),
                None => Err(ParseErrorKind::UnclosedBrace),
            };
        }
        if self.peek() == Some('$') {
            self.bump();
        }
        let name = self.name()?;
        self.skip_whitespace();
        if self.peek() == Some('}') {
            self.bump();
            parts.push(Part::Placeholder(name));
            return Ok(());
        }
        if self.rest().starts_with("->") {
            self.position += 2;
            parts.push(Part::Select(self.select(name, depth + 1)?));
            return Ok(());
        }
        match self.peek() {
            None => Err(ParseErrorKind::UnclosedBrace),
            Some(_) => Err(ParseErrorKind::InvalidPlaceholder),
        }
    }

    fn name(&mut self) -> Result<String, ParseErrorKind> {
        let start = self.position;
        while self.peek().is_some_and(is_name_char) {
            self.bump();
        }
        let name = &self.source[start..self.position];
        let starts_well = name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        match (starts_well, self.peek()) {
            (true, _) => Ok(name.to_string()),
            (false, None) => Err(ParseErrorKind::UnclosedBrace),
            (false, Some(_)) => Err(ParseErrorKind::InvalidPlaceholder),
        }
    }

    fn literal(&mut self) -> Result<String, ParseErrorKind> {
        let mut text = String::new();
        loop {
            match self.bump() {
                None => return Err(ParseErrorKind::UnclosedLiteral),
                Some('"') => return Ok(text),
                Some('\\') => match self.bump() {
                    Some(c) => text.push(c),
                    None => return Err(ParseErrorKind::UnclosedLiteral),
                },
                Some(c) => text.push(c),
            }
        }
    }

    fn select(&mut self, variable: String, depth: usize) -> Result<Select, ParseErrorKind> {
        if depth > MAX_DEPTH {
            return Err(ParseErrorKind::NestingTooDeep);
        }
        let mut variants: Vec<Variant> = Vec::new();
        let mut default = None;
        loop {
            self.skip_whitespace();
            match self.peek() {
                None => return Err(ParseErrorKind::UnclosedBrace),
                Some('}') => {
                    self.bump();
                    break;
                }
                Some(_) => {}
            }
            let Some((length, is_default, key)) = self.marker() else {
                return Err(ParseErrorKind::InvalidVariant);
            };
            let key = VariantKey::from_marker(key);
            if variants.iter().any(|v| v.key == key) {
                return Err(ParseErrorKind::DuplicateVariant);
            }
            if is_default && default.replace(variants.len()).is_some() {
                return Err(ParseErrorKind::MultipleDefaultVariants);
            }
            self.position += length;
            let mut pattern = self.pattern(depth, true)?;
            trim_variant(&mut pattern);
            variants.push(Variant { key, pattern });
        }
        let default = default.ok_or(ParseErrorKind::MissingDefaultVariant)?;
        Ok(Select {
            variable,
            variants,
            default,
        })
    }
}

/// Removes the whitespace around a variant's text.
fn trim_variant(parts: &mut Vec<Part>) {
    if let Some(Part::Text(text)) = parts.first_mut() {
        *text = text.trim_start().to_string();
    }
    if let Some(Part::Text(text)) = parts.last_mut() {
        *text = text.trim_end().to_string();
    }
    parts.retain(|part| !matches!(part, Part::Text(text) if text.is_empty()));
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
