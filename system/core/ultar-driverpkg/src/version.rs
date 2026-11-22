//! Semantic version numbers (`major.minor.patch[-pre][+build]`) with ordering.

use crate::error::Coded;
use std::cmp::Ordering;
use std::fmt;

/// One dot-separated part of a pre-release tag. Numeric parts sort below text parts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum PreId {
    Num(u64),
    Text(String),
}

/// A parsed semantic version. Build metadata (`+…`) is accepted and ignored.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Vec<PreId>,
}

/// Why a version string was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionError {
    /// The string does not have exactly three numeric parts.
    WrongShape(String),
    /// A numeric part is empty, not a number, too large or has a leading zero.
    BadNumber(String),
    /// The pre-release tag is empty or contains characters outside `[0-9A-Za-z-]`.
    BadPreRelease(String),
}

impl Coded for VersionError {
    fn code(&self) -> &'static str {
        match self {
            Self::WrongShape(_) => "driverpkg.version.wrong-shape",
            Self::BadNumber(_) => "driverpkg.version.bad-number",
            Self::BadPreRelease(_) => "driverpkg.version.bad-pre-release",
        }
    }
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (Self::WrongShape(s) | Self::BadNumber(s) | Self::BadPreRelease(s)) = self;
        write!(f, "{}: {s}", self.code())
    }
}

impl std::error::Error for VersionError {}

fn parse_number(part: &str, whole: &str) -> Result<u64, VersionError> {
    let plain = !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    let leading_zero = part.len() > 1 && part.starts_with('0');
    if !plain || leading_zero {
        return Err(VersionError::BadNumber(whole.to_string()));
    }
    part.parse()
        .map_err(|_| VersionError::BadNumber(whole.to_string()))
}

fn parse_pre(tag: &str, whole: &str) -> Result<Vec<PreId>, VersionError> {
    let mut parts = Vec::new();
    for part in tag.split('.') {
        let valid =
            !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !valid {
            return Err(VersionError::BadPreRelease(whole.to_string()));
        }
        match part.parse::<u64>() {
            Ok(n) if part.bytes().all(|b| b.is_ascii_digit()) => parts.push(PreId::Num(n)),
            _ => parts.push(PreId::Text(part.to_string())),
        }
    }
    Ok(parts)
}

impl Version {
    /// Creates a release version without pre-release tag.
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
            pre: Vec::new(),
        }
    }

    /// Parses a version string such as `1.2.0` or `2.0.0-rc.1`.
    pub fn parse(text: &str) -> Result<Self, VersionError> {
        let without_build = text.split('+').next().unwrap_or_default();
        let (core, pre) = match without_build.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (without_build, None),
        };
        let nums: Vec<&str> = core.split('.').collect();
        if nums.len() != 3 {
            return Err(VersionError::WrongShape(text.to_string()));
        }
        Ok(Self {
            major: parse_number(nums[0], text)?,
            minor: parse_number(nums[1], text)?,
            patch: parse_number(nums[2], text)?,
            pre: match pre {
                Some(tag) => parse_pre(tag, text)?,
                None => Vec::new(),
            },
        })
    }

    /// Returns `true` for a version carrying a pre-release tag.
    pub fn is_pre_release(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let core =
            (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch));
        if core != Ordering::Equal {
            return core;
        }
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => self.pre.cmp(&other.pre),
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        for (i, part) in self.pre.iter().enumerate() {
            f.write_str(if i == 0 { "-" } else { "." })?;
            match part {
                PreId::Num(n) => write!(f, "{n}")?,
                PreId::Text(t) => f.write_str(t)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn parses_and_prints() {
        assert_eq!(v("1.2.3"), Version::new(1, 2, 3));
        assert_eq!(v("1.2.3").to_string(), "1.2.3");
        assert_eq!(v("2.0.0-rc.1").to_string(), "2.0.0-rc.1");
        assert_eq!(v("1.0.0+build7"), v("1.0.0"));
        assert!(v("1.0.0-alpha").is_pre_release());
        assert!(!v("1.0.0").is_pre_release());
    }

    #[test]
    fn orders_like_semver() {
        let ordered = [
            "0.9.9",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.2.0",
            "1.10.0",
            "2.0.0",
        ];
        for pair in ordered.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
        assert_eq!(v("1.2.3").cmp(&v("1.2.3")), Ordering::Equal);
    }

    #[test]
    fn rejects_bad_input() {
        for s in ["", "1", "1.2", "1.2.3.4", "a.b.c"] {
            assert!(Version::parse(s).is_err(), "{s}");
        }
        assert!(matches!(
            Version::parse("1.2"),
            Err(VersionError::WrongShape(_))
        ));
        assert!(matches!(
            Version::parse("1.02.3"),
            Err(VersionError::BadNumber(_))
        ));
        assert!(matches!(
            Version::parse("1.x.3"),
            Err(VersionError::BadNumber(_))
        ));
        assert!(matches!(
            Version::parse("1..3"),
            Err(VersionError::BadNumber(_))
        ));
        assert!(matches!(
            Version::parse("99999999999999999999.0.0"),
            Err(VersionError::BadNumber(_))
        ));
        assert!(matches!(
            Version::parse("1.2.3-"),
            Err(VersionError::BadPreRelease(_))
        ));
        assert!(matches!(
            Version::parse("1.2.3-a..b"),
            Err(VersionError::BadPreRelease(_))
        ));
        assert!(matches!(
            Version::parse("1.2.3-a_b"),
            Err(VersionError::BadPreRelease(_))
        ));
        let err = Version::parse("1.2").unwrap_err();
        assert_eq!(err.to_string(), "driverpkg.version.wrong-shape: 1.2");
    }
}
