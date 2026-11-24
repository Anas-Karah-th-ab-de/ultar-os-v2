//! App ids: what a manifest may call an app, and the names derived from it.
//!
//! `docs/APP-RULES.md` §2: lower-case English with hyphens (`camera`,
//! `file-manager`); apps of other publishers `<publisher>.<app>`. The
//! launcher keeps an id short enough that every name derived from it fits
//! the limits of the places it goes to: the log service's output channel
//! (`app-<id>`, at most 32 bytes) and its record identifiers.

use std::fmt;

/// Longest app id in bytes (`"app-"` + id ≤ 32, the output channel's
/// limit for a service name).
pub const MAX_LEN: usize = 28;
/// Prefix of the name an app's output is stored under in the log service.
pub const LOG_PREFIX: &str = "app-";

/// A checked app id.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct AppId(String);

/// Why a text is not an app id.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InvalidAppId;

impl fmt::Display for InvalidAppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("launcher.app_id.invalid")
    }
}

impl std::error::Error for InvalidAppId {}

fn segment_ok(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes.last().is_some_and(|b| *b != b'-')
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && !segment.contains("--")
}

impl AppId {
    /// Checks `text`: 1–[`MAX_LEN`] bytes, one or two segments separated
    /// by a dot, each starting with a letter, `a-z 0-9 -`, no `--`, not
    /// ending with `-`.
    pub fn new(text: &str) -> Result<Self, InvalidAppId> {
        let segments: Vec<&str> = text.split('.').collect();
        if text.is_empty()
            || text.len() > MAX_LEN
            || segments.len() > 2
            || !segments.iter().all(|s| segment_ok(s))
        {
            return Err(InvalidAppId);
        }
        Ok(Self(text.to_owned()))
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The name the app's standard output and error are stored under in
    /// the log service: `app-<id>` with the dot as `_` (ids never contain
    /// `_`, so the mapping is one to one).
    pub fn log_name(&self) -> String {
        format!("{LOG_PREFIX}{}", self.0.replace('.', "_"))
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_after_the_app_rules_are_accepted() {
        for good in [
            "camera",
            "file-manager",
            "km.notes",
            "a",
            "office-sheet2",
            "x.y",
        ] {
            let id = AppId::new(good).unwrap();
            assert_eq!(id.as_str(), good);
            assert_eq!(id.to_string(), good);
        }
        let long = "a".repeat(MAX_LEN);
        assert!(AppId::new(&long).is_ok());
    }

    #[test]
    fn everything_else_is_refused() {
        let long = "a".repeat(MAX_LEN + 1);
        for bad in [
            "", "Camera", "1app", "-a", "a-", "a--b", "a.b.c", ".a", "a.", "a_b", "a b", "a/b",
            "..", "a..b", "ä", &long, "a\0",
        ] {
            assert_eq!(AppId::new(bad), Err(InvalidAppId), "{bad:?}");
        }
        assert_eq!(InvalidAppId.to_string(), "launcher.app_id.invalid");
    }

    #[test]
    fn log_names_fit_the_output_channel_and_are_distinct() {
        let long = AppId::new(&format!("{}.{}", "a".repeat(13), "b".repeat(14))).unwrap();
        assert!(ultar_service::log::output::valid_name(&long.log_name()));
        assert_eq!(AppId::new("km.notes").unwrap().log_name(), "app-km_notes");
        assert_ne!(
            AppId::new("km.notes").unwrap().log_name(),
            AppId::new("km-notes").unwrap().log_name()
        );
    }
}
