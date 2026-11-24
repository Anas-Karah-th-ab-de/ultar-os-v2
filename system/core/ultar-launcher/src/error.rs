//! Why a request to the launcher was refused – typed, with the bus error
//! code and a stable key for each.

use crate::manifest::ManifestError;
use crate::perm::Permission;
use crate::platform::Feature;
use crate::profile::ProfileError;
use crate::store::StateError;
use std::fmt;
use ultar_bus::messages::launcher as contract;

/// Why a package was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackageError {
    /// It could not be read or is too large.
    Read,
    /// No trusted key signed it (or the signature is broken).
    Signature,
    /// Its manifest breaks the rules.
    Manifest(ManifestError),
    /// Its manifest asks for more than its profile allows.
    Profile(ProfileError),
}

/// Why a request was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaunchError {
    /// No installed app has this id.
    UnknownApp,
    /// The manifest does not declare the permission or the program.
    NotDeclared,
    /// The permission cannot be granted to the app's profile.
    NotGrantable,
    /// A permission the instance needs is not granted.
    NotGranted(Permission),
    /// The package was refused.
    Package(PackageError),
    /// The kernel lacks a feature and the operator did not accept that.
    Platform(Feature),
    /// Too many instances run.
    Limit,
    /// The cell could not be started.
    StartFailed,
    /// The launcher is shutting down.
    ShuttingDown,
    /// The state could not be read or written.
    Storage(StateError),
    /// The request names something invalid.
    Invalid,
}

impl LaunchError {
    /// The error code of the bus contract.
    pub const fn code(self) -> u32 {
        match self {
            Self::UnknownApp => contract::ERROR_UNKNOWN_APP,
            Self::NotDeclared => contract::ERROR_NOT_DECLARED,
            Self::NotGrantable => contract::ERROR_NOT_GRANTABLE,
            Self::NotGranted(_) => contract::ERROR_NOT_GRANTED,
            Self::Package(_) => contract::ERROR_PACKAGE_REFUSED,
            Self::Platform(_) => contract::ERROR_PLATFORM,
            Self::Storage(StateError::Exhausted | StateError::TooMany) | Self::Limit => {
                contract::ERROR_LIMIT
            }
            Self::StartFailed => contract::ERROR_START_FAILED,
            Self::ShuttingDown => contract::ERROR_SHUTTING_DOWN,
            Self::Storage(_) => contract::ERROR_STORAGE,
            Self::Invalid => contract::ERROR_INVALID,
        }
    }

    /// Stable key (records, messages).
    pub const fn key(self) -> &'static str {
        match self {
            Self::UnknownApp => "launcher.error.unknown_app",
            Self::NotDeclared => "launcher.error.not_declared",
            Self::NotGrantable => "launcher.error.not_grantable",
            Self::NotGranted(_) => "launcher.error.not_granted",
            Self::Package(PackageError::Read) => "launcher.error.package_read",
            Self::Package(PackageError::Signature) => "launcher.error.signature",
            Self::Package(PackageError::Manifest(_)) => "launcher.error.manifest",
            Self::Package(PackageError::Profile(_)) => "launcher.error.profile",
            Self::Platform(_) => "launcher.error.platform",
            Self::Limit => "launcher.error.limit",
            Self::StartFailed => "launcher.error.start_failed",
            Self::ShuttingDown => "launcher.error.shutting_down",
            Self::Storage(_) => "launcher.error.storage",
            Self::Invalid => "launcher.error.invalid",
        }
    }
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotGranted(p) => write!(f, "{} ({p})", self.key()),
            Self::Package(PackageError::Manifest(m)) => write!(f, "{} ({m})", self.key()),
            Self::Platform(feature) => write!(f, "{} ({})", self.key(), feature.word()),
            Self::Storage(state) => write!(f, "{} ({state})", self.key()),
            _ => f.write_str(self.key()),
        }
    }
}

impl std::error::Error for LaunchError {}

impl From<StateError> for LaunchError {
    fn from(error: StateError) -> Self {
        Self::Storage(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_keys_are_distinct_enough() {
        let all = [
            LaunchError::UnknownApp,
            LaunchError::NotDeclared,
            LaunchError::NotGrantable,
            LaunchError::NotGranted(Permission::Background),
            LaunchError::Package(PackageError::Signature),
            LaunchError::Platform(Feature::Cell),
            LaunchError::Limit,
            LaunchError::StartFailed,
            LaunchError::ShuttingDown,
            LaunchError::Storage(StateError::Corrupt(1)),
            LaunchError::Invalid,
        ];
        let codes: std::collections::BTreeSet<u32> = all.iter().map(|e| e.code()).collect();
        assert_eq!(codes.len(), all.len());
        assert_eq!(
            LaunchError::Storage(StateError::Exhausted).code(),
            contract::ERROR_LIMIT
        );
        assert!(all
            .iter()
            .all(|e| e.to_string().starts_with("launcher.error.")));
        assert!(LaunchError::Platform(Feature::Net)
            .to_string()
            .ends_with("(net)"));
        let manifest = LaunchError::Package(PackageError::Manifest(ManifestError::Id));
        assert_eq!(manifest.code(), contract::ERROR_PACKAGE_REFUSED);
        assert!(manifest.to_string().contains("launcher.manifest.id"));
        assert_eq!(
            LaunchError::from(StateError::Digest),
            LaunchError::Storage(StateError::Digest)
        );
    }
}
