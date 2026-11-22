//! Error type of the profile crate.

use std::fmt;

/// Errors produced while reading or writing a device profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileError {
    /// The TOML text could not be parsed into a profile; carries the parser detail.
    Parse(String),
    /// The profile could not be written as TOML; carries the serializer detail.
    Serialize(String),
    /// The stored profile belongs to another device or kernel.
    DeviceMismatch {
        /// Device id of the running system.
        expected: String,
        /// Device id found in the stored profile.
        found: String,
    },
}

impl ProfileError {
    /// Stable message key for translation (see [`crate::msg`]).
    pub fn key(&self) -> &'static str {
        match self {
            ProfileError::Parse(_) => "profile.error.parse",
            ProfileError::Serialize(_) => "profile.error.serialize",
            ProfileError::DeviceMismatch { .. } => "profile.error.device_mismatch",
        }
    }
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileError::Parse(detail) | ProfileError::Serialize(detail) => {
                write!(f, "{}: {detail}", self.key())
            }
            ProfileError::DeviceMismatch { expected, found } => {
                write!(f, "{}: {expected} != {found}", self.key())
            }
        }
    }
}

impl std::error::Error for ProfileError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_starts_with_key() {
        let errors = [
            ProfileError::Parse("line 1".into()),
            ProfileError::Serialize("bad".into()),
            ProfileError::DeviceMismatch {
                expected: "a".into(),
                found: "b".into(),
            },
        ];
        for error in errors {
            assert!(error.to_string().starts_with(error.key()));
            assert!(crate::msg::text(error.key(), crate::msg::Lang::Ar).is_some());
        }
    }

    #[test]
    fn is_std_error() {
        let boxed: Box<dyn std::error::Error> = Box::new(ProfileError::Parse("x".into()));
        assert!(boxed.to_string().contains("x"));
    }
}
