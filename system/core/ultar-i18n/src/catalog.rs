//! Message catalogs: a set of keyed messages for one language.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;

use crate::error::{ParseError, ParseErrorKind};
use crate::parser;
use crate::pattern::Message;

/// A set of messages for one language, keyed by names such as `settings.display.title`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    messages: BTreeMap<String, Message>,
}

impl Catalog {
    /// Creates an empty catalog.
    pub fn new() -> Catalog {
        Catalog::default()
    }

    /// Parses a catalog from text in the Fluent-like format (see the crate's `INTEGRATION.md`).
    pub fn parse(source: &str) -> Result<Catalog, ParseError> {
        Catalog::parse_with_prefix(source, "")
    }

    /// Parses a catalog and puts every key under `namespace`, so `title` becomes `<namespace>.title`.
    ///
    /// Apps use this to bring their own catalogs without clashing with system keys.
    pub fn parse_namespaced(namespace: &str, source: &str) -> Result<Catalog, ParseError> {
        if !parser::is_valid_key(namespace) {
            return Err(ParseError::new(0, ParseErrorKind::InvalidNamespace, None));
        }
        Catalog::parse_with_prefix(source, &format!("{namespace}."))
    }

    fn parse_with_prefix(source: &str, prefix: &str) -> Result<Catalog, ParseError> {
        let mut messages = BTreeMap::new();
        for entry in parser::split_entries(source)? {
            let key = format!("{prefix}{}", entry.key);
            let parts = parser::parse_value(&entry.value)
                .map_err(|kind| ParseError::new(entry.line, kind, Some(key.clone())))?;
            if messages.contains_key(&key) {
                return Err(ParseError::new(
                    entry.line,
                    ParseErrorKind::DuplicateKey,
                    Some(key),
                ));
            }
            messages.insert(key, Message::from_parts(parts));
        }
        Ok(Catalog { messages })
    }

    /// Adds all messages of `other`; messages with the same key are replaced.
    pub fn merge(&mut self, other: Catalog) {
        self.messages.extend(other.messages);
    }

    /// Inserts or replaces a single message and returns the previous one.
    pub fn insert(&mut self, key: &str, message: Message) -> Option<Message> {
        self.messages.insert(String::from(key), message)
    }

    /// Looks up a message.
    pub fn get(&self, key: &str) -> Option<&Message> {
        self.messages.get(key)
    }

    /// `true` if the catalog has a message for `key`.
    pub fn contains(&self, key: &str) -> bool {
        self.messages.contains_key(key)
    }

    /// All keys in sorted order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.messages.keys().map(String::as_str)
    }

    /// All messages with their keys, sorted by key.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Message)> {
        self.messages
            .iter()
            .map(|(key, message)| (key.as_str(), message))
    }

    /// Keys that start with `prefix` (for example all keys of one namespace).
    pub fn keys_with_prefix<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = &'a str> {
        self.keys().filter(move |key| key.starts_with(prefix))
    }

    /// Number of messages.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// `true` if the catalog has no messages.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn parse_and_lookup() {
        let catalog = Catalog::parse("# c\nb = B\na = A {x}\n").unwrap();
        assert_eq!(catalog.len(), 2);
        assert!(!catalog.is_empty());
        assert!(catalog.contains("a"));
        assert!(!catalog.contains("c"));
        assert_eq!(catalog.keys().collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(
            catalog.get("b").and_then(Message::as_static_text),
            Some("B")
        );
        assert_eq!(catalog.iter().count(), 2);
        assert!(Catalog::new().is_empty());
        assert_eq!(Catalog::parse("").unwrap(), Catalog::new());
    }

    #[test]
    fn errors_carry_line_and_key() {
        let duplicate = Catalog::parse("a = 1\n\na = 2\n").unwrap_err();
        assert_eq!(duplicate.kind, ParseErrorKind::DuplicateKey);
        assert_eq!(duplicate.line, 3);
        assert_eq!(duplicate.key.as_deref(), Some("a"));
        let empty = Catalog::parse("a = ok\nb =\n").unwrap_err();
        assert_eq!((empty.kind, empty.line), (ParseErrorKind::EmptyValue, 2));
        let brace = Catalog::parse("x = 1\ny = {n ->\n [one] a\n").unwrap_err();
        let no_default = Catalog::parse("x = 1\n\ny = {n ->\n [one] a\n}\n").unwrap_err();
        assert_eq!(
            (no_default.kind, no_default.line),
            (ParseErrorKind::MissingDefaultVariant, 3)
        );
        assert_eq!((brace.kind, brace.line), (ParseErrorKind::UnclosedBrace, 2));
    }

    #[test]
    fn namespaces_prefix_keys() {
        let catalog = Catalog::parse_namespaced("settings.display", "title = T\n").unwrap();
        assert!(catalog.contains("settings.display.title"));
        assert_eq!(catalog.keys_with_prefix("settings.").count(), 1);
        assert_eq!(catalog.keys_with_prefix("apps.").count(), 0);
        for bad in ["", "a b", ".x", "x."] {
            let error = Catalog::parse_namespaced(bad, "title = T\n").unwrap_err();
            assert_eq!(error.kind, ParseErrorKind::InvalidNamespace);
        }
        let inner = Catalog::parse_namespaced("app", "t = {x").unwrap_err();
        assert_eq!(inner.key.as_deref(), Some("app.t"));
    }

    #[test]
    fn merge_and_insert_replace() {
        let mut base = Catalog::parse("a = 1\nb = 2\n").unwrap();
        base.merge(Catalog::parse("b = 3\nc = 4\n").unwrap());
        assert_eq!(base.len(), 3);
        assert_eq!(base.get("b").and_then(Message::as_static_text), Some("3"));
        let message = base.get("c").unwrap().clone();
        assert!(base.insert("d", message.clone()).is_none());
        assert_eq!(
            base.insert("a", message)
                .and_then(|m| m.as_static_text().map(String::from)),
            Some(String::from("1"))
        );
    }
}
