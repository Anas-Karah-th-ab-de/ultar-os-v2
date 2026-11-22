//! Typed hardware patterns used in the `[match]` section of a manifest.

use crate::error::Coded;
use std::fmt;

/// Why a hardware pattern was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternError {
    /// An id pattern is not of the form `vvvv:dddd` or `vvvv:*` (hexadecimal).
    BadId(String),
    /// A class prefix is not 2, 4 or 6 hexadecimal digits.
    BadClass(String),
    /// A name pattern (device-tree, ACPI, DMI, platform) is empty or has control characters.
    BadName(String),
}

impl Coded for PatternError {
    fn code(&self) -> &'static str {
        match self {
            Self::BadId(_) => "driverpkg.pattern.bad-id",
            Self::BadClass(_) => "driverpkg.pattern.bad-class",
            Self::BadName(_) => "driverpkg.pattern.bad-name",
        }
    }
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (Self::BadId(s) | Self::BadClass(s) | Self::BadName(s)) = self;
        write!(f, "{}: {s}", self.code())
    }
}

impl std::error::Error for PatternError {}

fn parse_hex16(part: &str) -> Option<u16> {
    if part.len() != 4 || !part.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u16::from_str_radix(part, 16).ok()
}

/// Parses a concrete `vvvv:dddd` pair (no wildcard), as used for device ids.
pub fn parse_id_pair(text: &str) -> Option<(u16, u16)> {
    let (vendor, device) = text.split_once(':')?;
    Some((parse_hex16(vendor)?, parse_hex16(device)?))
}

/// A PCI `vendor:device` or USB `vid:pid` pattern; the second part may be `*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IdPattern {
    /// Vendor id (always concrete).
    pub vendor: u16,
    /// Device/product id, `None` for the wildcard `*`.
    pub device: Option<u16>,
}

impl IdPattern {
    /// Parses `8086:0d4c` or `8086:*`.
    pub fn parse(text: &str) -> Result<Self, PatternError> {
        let bad = || PatternError::BadId(text.to_string());
        let (vendor, device) = text.split_once(':').ok_or_else(bad)?;
        let vendor = parse_hex16(vendor).ok_or_else(bad)?;
        let device = if device == "*" {
            None
        } else {
            Some(parse_hex16(device).ok_or_else(bad)?)
        };
        Ok(Self { vendor, device })
    }

    /// Returns `true` when the pattern has no wildcard.
    pub fn is_exact(&self) -> bool {
        self.device.is_some()
    }

    /// Checks a concrete id against the pattern.
    pub fn matches(&self, vendor: u16, device: u16) -> bool {
        self.vendor == vendor && self.device.is_none_or(|d| d == device)
    }
}

impl fmt::Display for IdPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.device {
            Some(device) => write!(f, "{:04x}:{device:04x}", self.vendor),
            None => write!(f, "{:04x}:*", self.vendor),
        }
    }
}

/// A prefix of a 24-bit class code (`class`, `class+subclass` or all three bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClassPrefix {
    value: u32,
    digits: u8,
}

impl ClassPrefix {
    /// Parses `0c`, `0c03` or `0c0330`.
    pub fn parse(text: &str) -> Result<Self, PatternError> {
        let valid = matches!(text.len(), 2 | 4 | 6) && text.bytes().all(|b| b.is_ascii_hexdigit());
        if !valid {
            return Err(PatternError::BadClass(text.to_string()));
        }
        let value =
            u32::from_str_radix(text, 16).map_err(|_| PatternError::BadClass(text.to_string()))?;
        Ok(Self {
            value,
            digits: text.len() as u8,
        })
    }

    /// Number of hexadecimal digits in the prefix (2, 4 or 6); longer is more specific.
    pub fn digits(&self) -> u8 {
        self.digits
    }

    /// The prefix of `class` with the given number of digits, if that length is valid.
    pub fn of_class(class: u32, digits: u8) -> Option<Self> {
        if !matches!(digits, 2 | 4 | 6) {
            return None;
        }
        let value = (class & 0x00ff_ffff) >> (24 - 4 * u32::from(digits));
        Some(Self { value, digits })
    }

    /// Checks whether a 24-bit class code starts with this prefix.
    pub fn matches(&self, class: u32) -> bool {
        Self::of_class(class, self.digits) == Some(*self)
    }
}

impl fmt::Display for ClassPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:0width$x}",
            self.value,
            width = usize::from(self.digits)
        )
    }
}

/// Validates a free-form name pattern (device-tree compatible, ACPI id, DMI model, platform name).
pub fn check_name(text: &str) -> Result<(), PatternError> {
    let valid = !text.is_empty()
        && text.len() <= 128
        && text == text.trim()
        && !text.chars().any(char::is_control);
    if valid {
        Ok(())
    } else {
        Err(PatternError::BadName(text.to_string()))
    }
}

/// Case-insensitive glob match where `*` stands for any (possibly empty) sequence.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let (mut p, mut t) = (0usize, 0usize);
    let mut backtrack: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == '*' {
            backtrack = Some((p, t));
            p += 1;
        } else if p < pattern.len() && pattern[p] == text[t] {
            p += 1;
            t += 1;
        } else if let Some((star, at)) = backtrack {
            backtrack = Some((star, at + 1));
            p = star + 1;
            t = at + 1;
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_patterns() {
        let exact = IdPattern::parse("8086:0D4C").unwrap();
        assert!(exact.is_exact());
        assert!(exact.matches(0x8086, 0x0d4c));
        assert!(!exact.matches(0x8086, 0x0d4d));
        assert_eq!(exact.to_string(), "8086:0d4c");
        let wild = IdPattern::parse("8086:*").unwrap();
        assert!(!wild.is_exact());
        assert!(wild.matches(0x8086, 0x1234));
        assert!(!wild.matches(0x10de, 0x1234));
        assert_eq!(wild.to_string(), "8086:*");
        for bad in [
            "",
            "8086",
            "8086:",
            "*:1234",
            "*:*",
            "808:1234",
            "8086:12345",
            "zzzz:0000",
            "8086:0d4c:1",
        ] {
            assert_eq!(
                IdPattern::parse(bad),
                Err(PatternError::BadId(bad.to_string())),
                "{bad}"
            );
        }
        assert_eq!(parse_id_pair("17aa:22be"), Some((0x17aa, 0x22be)));
        assert_eq!(parse_id_pair("17aa:*"), None);
    }

    #[test]
    fn class_prefixes() {
        let full = ClassPrefix::parse("0c0330").unwrap();
        assert!(full.matches(0x0c0330));
        assert!(!full.matches(0x0c0320));
        assert_eq!(full.digits(), 6);
        let sub = ClassPrefix::parse("0403").unwrap();
        assert!(sub.matches(0x040300) && sub.matches(0x040380) && !sub.matches(0x040100));
        let base = ClassPrefix::parse("03").unwrap();
        assert!(base.matches(0x030000) && base.matches(0x038000) && !base.matches(0x040000));
        assert_eq!(base.to_string(), "03");
        assert_eq!(sub.to_string(), "0403");
        assert_eq!(
            ClassPrefix::of_class(0x010802, 4),
            ClassPrefix::parse("0108").ok()
        );
        assert_eq!(ClassPrefix::of_class(0x010802, 3), None);
        for bad in ["", "0", "0c0", "0c03300", "xx", "0c 3"] {
            assert_eq!(
                ClassPrefix::parse(bad),
                Err(PatternError::BadClass(bad.to_string())),
                "{bad}"
            );
        }
    }

    #[test]
    fn names_and_globs() {
        assert!(check_name("samsung,exynos7885").is_ok());
        assert!(check_name("").is_err());
        assert!(check_name(" padded").is_err());
        assert!(check_name("line\nbreak").is_err());
        assert!(check_name(&"x".repeat(129)).is_err());
        assert!(glob_match("ThinkPad P1*", "thinkpad p1 gen 3"));
        assert!(glob_match("*GTi15*", "Beelink GTi15 Ultra"));
        assert!(glob_match("exact", "EXACT"));
        assert!(glob_match("*", ""));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"));
        assert!(!glob_match("ThinkPad", "ThinkPad P1"));
        assert!(!glob_match("", "x"));
        assert!(glob_match("", ""));
        assert_eq!(
            PatternError::BadName("x".into()).to_string(),
            "driverpkg.pattern.bad-name: x"
        );
    }
}
