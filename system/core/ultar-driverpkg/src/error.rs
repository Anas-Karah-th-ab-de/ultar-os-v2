//! Common error plumbing: stable error codes and the crate-wide [`Error`] wrapper.
//!
//! Every error type of this crate implements [`Coded`]. Its `Display` output is the
//! stable code followed by technical details; user-facing wording is looked up from
//! the code in [`crate::msg`], so no translated text lives inside the logic.

use crate::hwlist::HwListError;
use crate::index::IndexError;
use crate::keys::KeyError;
use crate::manifest::ManifestError;
use crate::package::PackageError;
use crate::pattern::PatternError;
use crate::sign::SignatureError;
use crate::version::VersionError;
use std::fmt;

/// An error that carries a stable, translatable message key.
pub trait Coded {
    /// The stable message key, e.g. `driverpkg.package.unsigned`.
    fn code(&self) -> &'static str;
}

/// Any error produced by this crate.
#[derive(Debug)]
pub enum Error {
    /// Key handling failed.
    Key(KeyError),
    /// A signature was rejected.
    Signature(SignatureError),
    /// A manifest was rejected.
    Manifest(ManifestError),
    /// A package was rejected.
    Package(PackageError),
    /// A depot index was rejected or could not be built.
    Index(IndexError),
    /// A hardware list file was rejected.
    HwList(HwListError),
    /// A hardware pattern was rejected.
    Pattern(PatternError),
    /// A version string was rejected.
    Version(VersionError),
}

impl Coded for Error {
    fn code(&self) -> &'static str {
        match self {
            Self::Key(e) => e.code(),
            Self::Signature(e) => e.code(),
            Self::Manifest(e) => e.code(),
            Self::Package(e) => e.code(),
            Self::Index(e) => e.code(),
            Self::HwList(e) => e.code(),
            Self::Pattern(e) => e.code(),
            Self::Version(e) => e.code(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key(e) => e.fmt(f),
            Self::Signature(e) => e.fmt(f),
            Self::Manifest(e) => e.fmt(f),
            Self::Package(e) => e.fmt(f),
            Self::Index(e) => e.fmt(f),
            Self::HwList(e) => e.fmt(f),
            Self::Pattern(e) => e.fmt(f),
            Self::Version(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Key(e) => Some(e),
            Self::Signature(e) => Some(e),
            Self::Manifest(e) => Some(e),
            Self::Package(e) => Some(e),
            Self::Index(e) => Some(e),
            Self::HwList(e) => Some(e),
            Self::Pattern(e) => Some(e),
            Self::Version(e) => Some(e),
        }
    }
}

macro_rules! wrap {
    ($($variant:ident($inner:ty)),* $(,)?) => {
        $(impl From<$inner> for Error {
            fn from(e: $inner) -> Self {
                Self::$variant(e)
            }
        })*
    };
}

wrap!(
    Key(KeyError),
    Signature(SignatureError),
    Manifest(ManifestError),
    Package(PackageError),
    Index(IndexError),
    HwList(HwListError),
    Pattern(PatternError),
    Version(VersionError),
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_and_keeps_code() {
        let inner = VersionError::WrongShape("1".into());
        let code = inner.code();
        let text = inner.to_string();
        let wrapped: Error = inner.into();
        assert_eq!(wrapped.code(), code);
        assert_eq!(wrapped.to_string(), text);
        assert!(std::error::Error::source(&wrapped).is_some());
        let wrapped: Error = PatternError::BadId("x".into()).into();
        assert_eq!(wrapped.code(), "driverpkg.pattern.bad-id");
        let wrapped: Error = KeyError::BadHex.into();
        assert_eq!(wrapped.code(), "driverpkg.key.bad-hex");
        let wrapped: Error = SignatureError::Invalid.into();
        assert_eq!(wrapped.code(), "driverpkg.signature.invalid");
    }
}
