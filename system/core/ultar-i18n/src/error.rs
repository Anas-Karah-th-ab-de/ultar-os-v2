//! Error types of the catalog parser.

use alloc::string::String;
use core::fmt;

/// What went wrong while parsing a catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParseErrorKind {
    /// The key is empty or contains characters other than letters, digits, `_`, `-` and `.`.
    InvalidKey,
    /// A line is neither a comment, a continuation nor a `key = text` entry.
    MissingEquals,
    /// The same key is defined twice in one source.
    DuplicateKey,
    /// The entry has no text.
    EmptyValue,
    /// A `{` is never closed.
    UnclosedBrace,
    /// A `}` appears without a matching `{`.
    UnexpectedClosingBrace,
    /// The content of `{…}` is not a placeholder name, a literal or a selection.
    InvalidPlaceholder,
    /// A `{"…"}` literal is never closed.
    UnclosedLiteral,
    /// A selection has no variant marked with `*` as default.
    MissingDefaultVariant,
    /// A selection has more than one default variant.
    MultipleDefaultVariants,
    /// The same variant key appears twice in a selection.
    DuplicateVariant,
    /// A selection contains text that is not introduced by a `[key]` variant marker.
    InvalidVariant,
    /// An indented continuation line appears before any entry.
    OrphanContinuation,
    /// Selections are nested deeper than the parser allows.
    NestingTooDeep,
    /// A namespace is empty or contains invalid characters.
    InvalidNamespace,
}

impl ParseErrorKind {
    /// All kinds, for tests and tools that enumerate them.
    pub const ALL: [ParseErrorKind; 15] = [
        ParseErrorKind::InvalidKey,
        ParseErrorKind::MissingEquals,
        ParseErrorKind::DuplicateKey,
        ParseErrorKind::EmptyValue,
        ParseErrorKind::UnclosedBrace,
        ParseErrorKind::UnexpectedClosingBrace,
        ParseErrorKind::InvalidPlaceholder,
        ParseErrorKind::UnclosedLiteral,
        ParseErrorKind::MissingDefaultVariant,
        ParseErrorKind::MultipleDefaultVariants,
        ParseErrorKind::DuplicateVariant,
        ParseErrorKind::InvalidVariant,
        ParseErrorKind::OrphanContinuation,
        ParseErrorKind::NestingTooDeep,
        ParseErrorKind::InvalidNamespace,
    ];

    /// Message key that describes this kind in the catalogs (`i18n.parse.*`).
    pub const fn message_key(self) -> &'static str {
        match self {
            ParseErrorKind::InvalidKey => "i18n.parse.invalid_key",
            ParseErrorKind::MissingEquals => "i18n.parse.missing_equals",
            ParseErrorKind::DuplicateKey => "i18n.parse.duplicate_key",
            ParseErrorKind::EmptyValue => "i18n.parse.empty_value",
            ParseErrorKind::UnclosedBrace => "i18n.parse.unclosed_brace",
            ParseErrorKind::UnexpectedClosingBrace => "i18n.parse.unexpected_closing_brace",
            ParseErrorKind::InvalidPlaceholder => "i18n.parse.invalid_placeholder",
            ParseErrorKind::UnclosedLiteral => "i18n.parse.unclosed_literal",
            ParseErrorKind::MissingDefaultVariant => "i18n.parse.missing_default_variant",
            ParseErrorKind::MultipleDefaultVariants => "i18n.parse.multiple_default_variants",
            ParseErrorKind::DuplicateVariant => "i18n.parse.duplicate_variant",
            ParseErrorKind::InvalidVariant => "i18n.parse.invalid_variant",
            ParseErrorKind::OrphanContinuation => "i18n.parse.orphan_continuation",
            ParseErrorKind::NestingTooDeep => "i18n.parse.nesting_too_deep",
            ParseErrorKind::InvalidNamespace => "i18n.parse.invalid_namespace",
        }
    }

    /// Short technical description for logs (not shown to users; use [`Self::message_key`] for that).
    const fn technical(self) -> &'static str {
        match self {
            ParseErrorKind::InvalidKey => "invalid key",
            ParseErrorKind::MissingEquals => "missing '='",
            ParseErrorKind::DuplicateKey => "duplicate key",
            ParseErrorKind::EmptyValue => "empty value",
            ParseErrorKind::UnclosedBrace => "unclosed '{'",
            ParseErrorKind::UnexpectedClosingBrace => "unexpected '}'",
            ParseErrorKind::InvalidPlaceholder => "invalid placeholder",
            ParseErrorKind::UnclosedLiteral => "unclosed literal",
            ParseErrorKind::MissingDefaultVariant => "missing default variant",
            ParseErrorKind::MultipleDefaultVariants => "multiple default variants",
            ParseErrorKind::DuplicateVariant => "duplicate variant",
            ParseErrorKind::InvalidVariant => "invalid variant",
            ParseErrorKind::OrphanContinuation => "continuation line without entry",
            ParseErrorKind::NestingTooDeep => "nesting too deep",
            ParseErrorKind::InvalidNamespace => "invalid namespace",
        }
    }
}

/// A catalog parse error with its position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line on which the faulty entry starts (0 if the error is not tied to a line).
    pub line: usize,
    /// What went wrong.
    pub kind: ParseErrorKind,
    /// The key of the faulty entry, if it could be read.
    pub key: Option<String>,
}

impl ParseError {
    /// Creates an error for an entry starting on `line`.
    pub fn new(line: usize, kind: ParseErrorKind, key: Option<String>) -> ParseError {
        ParseError { line, kind, key }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.kind.technical())?;
        match &self.key {
            Some(key) => write!(f, " ({key})"),
            None => Ok(()),
        }
    }
}

impl core::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeSet;
    use alloc::format;
    use alloc::string::ToString;

    #[test]
    fn every_kind_has_a_distinct_message_key() {
        let keys: BTreeSet<&str> = ParseErrorKind::ALL
            .iter()
            .map(|k| k.message_key())
            .collect();
        assert_eq!(keys.len(), ParseErrorKind::ALL.len());
        assert!(keys.iter().all(|k| k.starts_with("i18n.parse.")));
    }

    #[test]
    fn display_names_line_kind_and_key() {
        let with_key = ParseError::new(7, ParseErrorKind::DuplicateKey, Some("a.b".to_string()));
        assert_eq!(format!("{with_key}"), "line 7: duplicate key (a.b)");
        let without = ParseError::new(3, ParseErrorKind::MissingEquals, None);
        assert_eq!(format!("{without}"), "line 3: missing '='");
        for kind in ParseErrorKind::ALL {
            assert!(!format!("{}", ParseError::new(1, kind, None)).is_empty());
        }
    }
}
