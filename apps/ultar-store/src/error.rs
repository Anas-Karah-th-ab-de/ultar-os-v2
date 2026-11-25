//! The app's own error type; every variant names an i18n key (APP-RULES §8).

use std::fmt;

/// What can go wrong on a catalogue source. Defined in the shared crate `ultar-catalog`
/// (the shell's search reads catalogues with the same errors); re-exported here.
pub use ultar_catalog::source::SourceError;

/// What can go wrong on an operation with the system (launcher, files).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    /// The launcher service is not on the bus.
    NoLauncher,
    /// The launcher refused: the package signature is invalid or unknown (bus error 4).
    PackageRefused,
    /// The launcher refused: no installed app has this id (bus error 1).
    UnknownApp,
    /// The launcher refused: the cell could not be started (bus error 7).
    StartFailed,
    /// The launcher is shutting down (bus error 8).
    ShuttingDown,
    /// The launcher's state could not be stored (bus error 9).
    Storage,
    /// The request names something invalid (app id, path; bus error 11).
    Invalid,
    /// Not enough free space. The launcher contract has no error code for it yet
    /// (open point in docs/integration/ultar-store.md); the Store answers with
    /// the "what can be freed" surface, not with an error message.
    NoSpace {
        /// Bytes needed; `None` = computed at run time (`—` on the board).
        required: Option<u64>,
        /// Bytes free; `None` = computed at run time.
        available: Option<u64>,
    },
    /// The call timed out or the transport failed.
    Unreachable,
    /// A file could not be read or written.
    Io,
    /// The service answered with something this app cannot understand.
    Protocol,
    /// The app is a base app of the system and stays installed (bus error 14).
    Protected,
}

/// Everything the Store can fail with. Each variant maps to one catalog key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// Loading the catalogue failed.
    Source(SourceError),
    /// An operation through the launcher or the files failed.
    Backend(BackendError),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(e) => write!(f, "catalogue: {e:?}"),
            Self::Backend(e) => write!(f, "backend: {e:?}"),
        }
    }
}

impl From<SourceError> for StoreError {
    fn from(e: SourceError) -> Self {
        Self::Source(e)
    }
}

impl From<BackendError> for StoreError {
    fn from(e: BackendError) -> Self {
        Self::Backend(e)
    }
}

impl StoreError {
    /// The i18n key that explains this error in plain words (Arabic first).
    pub fn message_key(&self) -> &'static str {
        match self {
            Self::Source(SourceError::Unreadable) => "store.error.unreadable",
            Self::Source(SourceError::Damaged) => "store.error.damaged",
            Self::Source(SourceError::Unsigned) => "store.error.unsigned",
            Self::Source(SourceError::BadSignature) => "store.error.bad-signature",
            Self::Source(SourceError::UnknownSigner) => "store.error.unknown-signer",
            Self::Source(SourceError::TooNew) => "store.error.too-new",
            Self::Source(SourceError::PackageMismatch) => "store.error.package-mismatch",
            Self::Backend(BackendError::NoLauncher) => "store.error.no-launcher",
            Self::Backend(BackendError::PackageRefused) => "store.error.package-refused",
            Self::Backend(BackendError::UnknownApp) => "store.error.unknown-app",
            Self::Backend(BackendError::StartFailed) => "store.error.start-failed",
            Self::Backend(BackendError::ShuttingDown) => "store.error.shutting-down",
            Self::Backend(BackendError::Storage) => "store.error.storage",
            Self::Backend(BackendError::Invalid) => "store.error.invalid",
            // The free-space surface explains itself with its own keys.
            Self::Backend(BackendError::NoSpace { .. }) => "store.nospace.title",
            Self::Backend(BackendError::Unreachable) => "store.error.unreachable",
            Self::Backend(BackendError::Io) => "store.error.io",
            Self::Backend(BackendError::Protocol) => "store.error.protocol",
            Self::Backend(BackendError::Protected) => "store.error.protected",
        }
    }

    /// True when the device is offline and the catalogue simply could not be reached;
    /// the offline surface is shown instead of the error surface (DESKTOP-SPEC §7.3).
    pub fn is_offline(&self) -> bool {
        matches!(
            self,
            Self::Source(SourceError::Unreadable) | Self::Backend(BackendError::Unreachable)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n;
    use ultar_i18n::Lang;

    #[test]
    fn every_error_key_exists_in_every_language() {
        let all = [
            StoreError::Source(SourceError::Unreadable),
            StoreError::Source(SourceError::Damaged),
            StoreError::Source(SourceError::Unsigned),
            StoreError::Source(SourceError::BadSignature),
            StoreError::Source(SourceError::UnknownSigner),
            StoreError::Source(SourceError::TooNew),
            StoreError::Source(SourceError::PackageMismatch),
            StoreError::Backend(BackendError::NoLauncher),
            StoreError::Backend(BackendError::PackageRefused),
            StoreError::Backend(BackendError::UnknownApp),
            StoreError::Backend(BackendError::StartFailed),
            StoreError::Backend(BackendError::ShuttingDown),
            StoreError::Backend(BackendError::Storage),
            StoreError::Backend(BackendError::Invalid),
            StoreError::Backend(BackendError::Unreachable),
            StoreError::Backend(BackendError::Io),
            StoreError::Backend(BackendError::Protocol),
        ];
        for lang in Lang::ALL {
            let tr = i18n::translator(lang).unwrap();
            for e in &all {
                let key = e.message_key();
                assert!(tr.catalog(lang).contains(key), "{key} missing in {lang:?}");
                assert_ne!(tr.t(key), key, "{key} resolves to itself in {lang:?}");
            }
        }
    }

    #[test]
    fn offline_classification() {
        assert!(StoreError::from(SourceError::Unreadable).is_offline());
        assert!(StoreError::from(BackendError::Unreachable).is_offline());
        assert!(!StoreError::from(SourceError::Damaged).is_offline());
        assert!(!StoreError::from(BackendError::NoLauncher).is_offline());
    }
}
