//! Typed errors of the signing service and its client.

use std::fmt;

/// Everything that can go wrong while signing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignError {
    /// The request line is malformed (unknown operation, wrong digest length, too long).
    BadRequest,
    /// The key of this role is missing or unreadable.
    KeyMissing,
    /// A key file is readable or writable by others than its owner, or is not a regular file.
    KeyPermissions,
    /// A key file does not hold exactly one 32-byte hex private key.
    KeyDamaged,
    /// The service was started as root (refused: it must run as its own user).
    RunningAsRoot,
    /// The rate limit of this key is used up; try again later.
    RateLimited,
    /// The audit log could not be written (then nothing is signed: fail closed).
    AuditFailed,
    /// Reading or writing a file or the socket failed.
    Io,
    /// The service answered something the client does not understand.
    Protocol,
    /// The service refused the request with this code.
    Refused(String),
    /// A package file to be signed is malformed (missing manifest or payload, duplicates).
    BadPackage,
}

impl SignError {
    /// The stable wire code of this error (`err <code>` on the socket).
    pub fn code(&self) -> &str {
        match self {
            Self::BadRequest => "bad_request",
            Self::KeyMissing => "key_missing",
            Self::KeyPermissions => "key_permissions",
            Self::KeyDamaged => "key_damaged",
            Self::RunningAsRoot => "running_as_root",
            Self::RateLimited => "rate_limited",
            Self::AuditFailed => "audit_failed",
            Self::Io => "io",
            Self::Protocol => "protocol",
            Self::Refused(code) => code,
            Self::BadPackage => "bad_package",
        }
    }
}

impl fmt::Display for SignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for SignError {}

impl From<std::io::Error> for SignError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_and_display_matches() {
        assert_eq!(SignError::RateLimited.code(), "rate_limited");
        assert_eq!(SignError::Refused("x".into()).to_string(), "x");
        assert_eq!(SignError::from(std::io::Error::other("e")), SignError::Io);
        for e in [
            SignError::BadRequest,
            SignError::KeyMissing,
            SignError::KeyPermissions,
            SignError::KeyDamaged,
            SignError::RunningAsRoot,
            SignError::AuditFailed,
            SignError::Protocol,
            SignError::BadPackage,
        ] {
            assert!(!e.code().is_empty());
            assert!(e.code().chars().all(|c| c.is_ascii_lowercase() || c == '_'));
        }
    }
}
