//! Release versions: `major.minor.patch`, compared numerically.

use std::fmt;

/// A release version. Ordering is numeric per component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Incompatible changes.
    pub major: u32,
    /// New functions.
    pub minor: u32,
    /// Corrections.
    pub patch: u32,
}

impl Version {
    /// Builds a version from its three numbers.
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parses the strict form `1.2.3`: three decimal numbers below 2^32 without sign, space or
    /// leading zero. `None` for everything else.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let major = number(parts.next()?)?;
        let minor = number(parts.next()?)?;
        let patch = number(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self::new(major, minor, patch))
    }
}

fn number(text: &str) -> Option<u32> {
    let plain = !text.is_empty()
        && text.len() <= 10
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text.len() == 1 || !text.starts_with('0'));
    if plain {
        text.parse().ok()
    } else {
        None
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_prints() {
        let v = Version::parse("0.7.12").unwrap();
        assert_eq!(v, Version::new(0, 7, 12));
        assert_eq!(v.to_string(), "0.7.12");
        assert_eq!(Version::parse("4294967295.0.0").unwrap().major, u32::MAX);
    }

    #[test]
    fn refuses_everything_else() {
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "1.2.x",
            "1..3",
            " 1.2.3",
            "1.2.3 ",
            "+1.2.3",
            "-1.2.3",
            "1.2.3-rc1",
            "4294967296.0.0",
            "1.2.３",
        ] {
            assert!(Version::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn orders_numerically() {
        assert!(Version::new(0, 10, 0) > Version::new(0, 9, 9));
        assert!(Version::new(1, 0, 0) > Version::new(0, 99, 99));
        assert!(Version::new(0, 7, 1) > Version::new(0, 7, 0));
    }
}
