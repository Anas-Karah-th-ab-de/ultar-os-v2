//! Hierarchical names (`devices.capabilities`) and subscription patterns
//! (`devices.*`, `*`).

use crate::error::BusError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Longest accepted name or pattern in bytes.
pub const MAX_NAME_LEN: usize = 128;

fn is_valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
}

/// `true` when `name` is a dot separated list of non-empty segments made of
/// `a-z`, `0-9`, `_` and `-`, at most [`MAX_NAME_LEN`] bytes long.
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME_LEN && name.split('.').all(is_valid_segment)
}

/// `true` when `name` is exactly one valid segment (used for method names).
pub fn is_valid_segment_name(name: &str) -> bool {
    name.len() <= MAX_NAME_LEN && is_valid_segment(name)
}

/// Returns `name` unchanged when it is a valid name.
pub fn check_name(name: &str) -> Result<&str, BusError> {
    if is_valid_name(name) {
        Ok(name)
    } else {
        Err(BusError::InvalidName)
    }
}

/// A subscription or permission pattern.
///
/// * `devices.capabilities` matches exactly that name,
/// * `devices.*` matches every name below `devices.` (any depth) but not
///   `devices` itself,
/// * `*` matches every name.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Pattern {
    /// Exact name, or the prefix in front of `.*` (empty for `*`).
    stem: String,
    /// Whether the pattern ends in a wildcard.
    wildcard: bool,
}

impl Pattern {
    /// Parses the textual form of a pattern.
    pub fn parse(text: &str) -> Result<Self, BusError> {
        if text.len() > MAX_NAME_LEN {
            return Err(BusError::InvalidName);
        }
        if text == "*" {
            return Ok(Self {
                stem: String::new(),
                wildcard: true,
            });
        }
        if let Some(stem) = text.strip_suffix(".*") {
            check_name(stem)?;
            return Ok(Self {
                stem: stem.to_owned(),
                wildcard: true,
            });
        }
        check_name(text)?;
        Ok(Self {
            stem: text.to_owned(),
            wildcard: false,
        })
    }

    /// The pattern matching every name.
    pub fn any() -> Self {
        Self {
            stem: String::new(),
            wildcard: true,
        }
    }

    /// Whether the concrete `name` is matched by this pattern.
    pub fn matches(&self, name: &str) -> bool {
        if !self.wildcard {
            return self.stem == name;
        }
        self.stem.is_empty() || is_below(name, &self.stem)
    }

    /// Whether every name matched by `other` is also matched by `self`.
    pub fn covers(&self, other: &Pattern) -> bool {
        if !other.wildcard {
            return self.matches(&other.stem);
        }
        self.wildcard
            && (self.stem.is_empty()
                || self.stem == other.stem
                || is_below(&other.stem, &self.stem))
    }

    /// Whether the pattern contains a wildcard.
    pub fn is_wildcard(&self) -> bool {
        self.wildcard
    }
}

fn is_below(name: &str, stem: &str) -> bool {
    name.len() > stem.len() + 1
        && name.starts_with(stem)
        && name.as_bytes().get(stem.len()) == Some(&b'.')
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.wildcard, self.stem.is_empty()) {
            (false, _) => f.write_str(&self.stem),
            (true, true) => f.write_str("*"),
            (true, false) => write!(f, "{}.*", self.stem),
        }
    }
}

impl TryFrom<String> for Pattern {
    type Error = BusError;

    fn try_from(text: String) -> Result<Self, BusError> {
        Self::parse(&text)
    }
}

impl From<Pattern> for String {
    fn from(pattern: Pattern) -> String {
        pattern.to_string()
    }
}

impl std::str::FromStr for Pattern {
    type Err = BusError;

    fn from_str(text: &str) -> Result<Self, BusError> {
        Self::parse(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(text: &str) -> Pattern {
        Pattern::parse(text).unwrap()
    }

    #[test]
    fn names_are_validated() {
        for good in [
            "a",
            "devices.capabilities",
            "app.tech-code_craft.phone1",
            "x.y.z",
        ] {
            assert!(is_valid_name(good), "{good}");
        }
        let long = "a".repeat(MAX_NAME_LEN + 1);
        for bad in [
            "",
            ".",
            "a.",
            ".a",
            "a..b",
            "A",
            "a b",
            "a.*",
            "*",
            "ä",
            long.as_str(),
        ] {
            assert!(!is_valid_name(bad), "{bad}");
        }
        assert!(is_valid_segment_name("get"));
        assert!(!is_valid_segment_name("a.b"));
        assert!(!is_valid_segment_name(""));
        assert!(check_name("ok").is_ok());
        assert!(matches!(check_name("NO"), Err(BusError::InvalidName)));
    }

    #[test]
    fn patterns_parse_and_print() {
        for text in ["*", "devices.*", "devices.capabilities", "a.b.*"] {
            assert_eq!(pattern(text).to_string(), text);
            assert_eq!(text.parse::<Pattern>().unwrap(), pattern(text));
        }
        let long = "a".repeat(MAX_NAME_LEN + 1);
        for bad in ["", "**", "a.**", "*.a", "a.*.b", ".*", "A.*", long.as_str()] {
            assert!(Pattern::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(Pattern::any(), pattern("*"));
        assert!(pattern("a.*").is_wildcard());
        assert!(!pattern("a").is_wildcard());
    }

    #[test]
    fn matching_follows_the_hierarchy() {
        assert!(pattern("*").matches("anything.at.all"));
        assert!(pattern("devices.*").matches("devices.capabilities"));
        assert!(pattern("devices.*").matches("devices.usb.added"));
        assert!(!pattern("devices.*").matches("devices"));
        assert!(!pattern("devices.*").matches("devicesx.y"));
        assert!(!pattern("devices.*").matches("power.state"));
        assert!(pattern("power.state").matches("power.state"));
        assert!(!pattern("power.state").matches("power.state.x"));
    }

    #[test]
    fn covering_is_a_partial_order() {
        assert!(pattern("*").covers(&pattern("a.*")));
        assert!(pattern("*").covers(&pattern("a")));
        assert!(pattern("a.*").covers(&pattern("a.*")));
        assert!(pattern("a.*").covers(&pattern("a.b.*")));
        assert!(pattern("a.*").covers(&pattern("a.b")));
        assert!(!pattern("a.*").covers(&pattern("a")));
        assert!(!pattern("a.b.*").covers(&pattern("a.*")));
        assert!(!pattern("a.*").covers(&pattern("*")));
        assert!(!pattern("a").covers(&pattern("a.*")));
        assert!(pattern("a").covers(&pattern("a")));
        assert!(!pattern("a.*").covers(&pattern("ab.*")));
    }

    #[test]
    fn patterns_serialise_as_strings() {
        let bytes = postcard::to_allocvec(&pattern("devices.*")).unwrap();
        assert_eq!(
            postcard::from_bytes::<Pattern>(&bytes).unwrap(),
            pattern("devices.*")
        );
        let bad = postcard::to_allocvec("NOT VALID").unwrap();
        assert!(postcard::from_bytes::<Pattern>(&bad).is_err());
    }
}
