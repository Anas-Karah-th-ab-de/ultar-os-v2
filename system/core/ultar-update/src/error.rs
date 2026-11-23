//! The one error type of the updater. Every variant maps to an i18n key; no text is built here.

use crate::manifest::Role;
use std::fmt;
use std::io;

/// Why a document (manifest, key set, envelope, bundle, slot table) could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Malformed {
    /// Wrong magic bytes.
    Magic,
    /// A length field exceeds its limit or the input ends early.
    Length,
    /// Not the canonical text form (order, characters, duplicates, unknown lines).
    Text,
    /// A number, version, hash or identifier inside the text is invalid.
    Field,
    /// Bytes follow where the document must end.
    Trailing,
}

/// Failure of the network source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkError {
    /// No connection, name not resolved, TLS failure, time-out, connection lost.
    Transport,
    /// The server answered with an unexpected status.
    Status(u16),
    /// The answer breaks the protocol (missing field, bad range, bad redirect).
    Protocol,
}

/// Everything that can go wrong in the updater.
#[derive(Debug)]
#[non_exhaustive]
pub enum UpdateError {
    /// File system failure.
    Io(io::Error),
    /// A signed document is not well formed.
    Malformed(Malformed),
    /// The device has no update trust keys at all.
    NoTrustedKeys,
    /// The signature was made with a key the device does not trust.
    UnknownKey,
    /// The signature does not match.
    BadSignature,
    /// The manifest is past its expiry time.
    Expired,
    /// Built for another processor architecture.
    WrongArch,
    /// Built for another edition.
    WrongEdition,
    /// This device is not in the manifest's device list.
    WrongDevice,
    /// The manifest belongs to another channel than the one asked for.
    WrongChannel,
    /// The payload roles differ from what the slots of this device hold.
    PayloadSetMismatch,
    /// The rollback index is below the one stored on the device.
    RollbackRefused {
        /// Index stored on the device.
        floor: u64,
        /// Index of the offered release.
        offered: u64,
    },
    /// The running version is older than the manifest's minimum.
    NeedsIntermediate,
    /// The offered version is the running one.
    UpToDate,
    /// The offered version is older than the running one.
    DowngradeRefused,
    /// A payload does not have the signed SHA-256.
    HashMismatch(Role),
    /// A payload read back from the slot differs from what was written.
    ReadBackMismatch(Role),
    /// More bytes arrived than the signed size.
    SizeExceeded,
    /// The stream ended before the signed size; the download can be resumed.
    Truncated,
    /// A rotation is older than the key set in force, or not signed by it.
    RotationRefused,
    /// Stored trust or anti-rollback state is damaged; nothing is accepted.
    StateCorrupt,
    /// Not enough free space for the download.
    NoSpace {
        /// Bytes needed including the reserve.
        needed: u64,
        /// Bytes free.
        free: u64,
    },
    /// No complete verified bundle is staged.
    NotStaged,
    /// Another operation is running, or a new version waits for its first start.
    Busy,
    /// The operation was cancelled.
    Cancelled,
    /// The operation was interrupted (injected fault; equals a power loss).
    Interrupted,
    /// No slot can be started.
    NoBootableSlot,
    /// The slot table is unreadable in both copies.
    BootControl,
    /// There is no previous version that may be started.
    PreviousUnavailable,
    /// There is no update for this device.
    NoUpdate,
    /// The network is metered and the policy does not allow that.
    Metered,
    /// The caller may not do this.
    PermissionDenied,
    /// The channel needs a signed-in account.
    AuthRequired,
    /// The account has no access to the channel.
    NotEntitled,
    /// The channel does not exist on the server.
    UnknownChannel,
    /// The server asks to slow down.
    RateLimited,
    /// Network failure.
    Network(NetworkError),
    /// The new system did not pass its health checks.
    Unhealthy,
    /// A policy or request value is out of range.
    InvalidArgument,
    /// The new release did not start; the boot code returned to the previous system.
    BootFailed,
    /// This CPU cannot run the TLS crypto provider; `https` servers cannot be reached.
    TlsUnavailable,
    /// The device was installed before A/B slots existed (one root partition, no
    /// `ULTAR-BOOTCTL`): there is no second slot to write to. Reinstalling is the way forward.
    LayoutNotAb,
}

impl UpdateError {
    /// The i18n key of the message for this error (`locales/*.ftl`).
    pub fn key(&self) -> &'static str {
        match self {
            Self::Io(_) => "update.error.io",
            Self::Malformed(_) => "update.error.malformed",
            Self::NoTrustedKeys => "update.error.no_trusted_keys",
            Self::UnknownKey => "update.error.unknown_key",
            Self::BadSignature => "update.error.bad_signature",
            Self::Expired => "update.error.expired",
            Self::WrongArch => "update.error.wrong_arch",
            Self::WrongEdition => "update.error.wrong_edition",
            Self::WrongDevice => "update.error.wrong_device",
            Self::WrongChannel => "update.error.wrong_channel",
            Self::PayloadSetMismatch => "update.error.payload_set",
            Self::RollbackRefused { .. } => "update.error.rollback_refused",
            Self::NeedsIntermediate => "update.error.needs_intermediate",
            Self::UpToDate => "update.error.up_to_date",
            Self::DowngradeRefused => "update.error.downgrade_refused",
            Self::HashMismatch(_) => "update.error.hash_mismatch",
            Self::ReadBackMismatch(_) => "update.error.read_back",
            Self::SizeExceeded => "update.error.size_exceeded",
            Self::Truncated => "update.error.truncated",
            Self::RotationRefused => "update.error.rotation_refused",
            Self::StateCorrupt => "update.error.state_corrupt",
            Self::NoSpace { .. } => "update.error.no_space",
            Self::NotStaged => "update.error.not_staged",
            Self::Busy => "update.error.busy",
            Self::Cancelled => "update.error.cancelled",
            Self::Interrupted => "update.error.interrupted",
            Self::NoBootableSlot => "update.error.no_bootable_slot",
            Self::BootControl => "update.error.boot_control",
            Self::PreviousUnavailable => "update.error.previous_unavailable",
            Self::NoUpdate => "update.error.no_update",
            Self::Metered => "update.error.metered",
            Self::PermissionDenied => "update.error.permission_denied",
            Self::AuthRequired => "update.error.auth_required",
            Self::NotEntitled => "update.error.not_entitled",
            Self::UnknownChannel => "update.error.unknown_channel",
            Self::RateLimited => "update.error.rate_limited",
            Self::Network(_) => "update.error.network",
            Self::Unhealthy => "update.error.unhealthy",
            Self::InvalidArgument => "update.error.invalid_argument",
            Self::BootFailed => "update.error.boot_failed",
            Self::TlsUnavailable => "update.error.tls_unavailable",
            Self::LayoutNotAb => "update.error.layout_not_ab",
        }
    }

    /// All message keys an error can produce (for catalogue completeness checks).
    pub const KEYS: [&'static str; 42] = [
        "update.error.io",
        "update.error.malformed",
        "update.error.no_trusted_keys",
        "update.error.unknown_key",
        "update.error.bad_signature",
        "update.error.expired",
        "update.error.wrong_arch",
        "update.error.wrong_edition",
        "update.error.wrong_device",
        "update.error.wrong_channel",
        "update.error.payload_set",
        "update.error.rollback_refused",
        "update.error.needs_intermediate",
        "update.error.up_to_date",
        "update.error.downgrade_refused",
        "update.error.hash_mismatch",
        "update.error.read_back",
        "update.error.size_exceeded",
        "update.error.truncated",
        "update.error.rotation_refused",
        "update.error.state_corrupt",
        "update.error.no_space",
        "update.error.not_staged",
        "update.error.busy",
        "update.error.cancelled",
        "update.error.interrupted",
        "update.error.no_bootable_slot",
        "update.error.boot_control",
        "update.error.previous_unavailable",
        "update.error.no_update",
        "update.error.metered",
        "update.error.permission_denied",
        "update.error.auth_required",
        "update.error.not_entitled",
        "update.error.unknown_channel",
        "update.error.rate_limited",
        "update.error.network",
        "update.error.unhealthy",
        "update.error.invalid_argument",
        "update.error.boot_failed",
        "update.error.tls_unavailable",
        "update.error.layout_not_ab",
    ];

    /// Stable numeric code used on the bus (`ErrorCode::Service(code)`): the position of the
    /// key in [`UpdateError::KEYS`].
    pub fn code(&self) -> u32 {
        let key = self.key();
        Self::KEYS
            .iter()
            .position(|k| *k == key)
            .map_or(0, |index| index as u32)
    }

    /// The message key for a bus code, if the code is known.
    pub fn key_of_code(code: u32) -> Option<&'static str> {
        Self::KEYS.get(code as usize).copied()
    }
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Diagnostic form for logs; user-visible text comes from `key()`.
        match self {
            Self::Io(e) => write!(f, "{}: {e}", self.key()),
            Self::Malformed(m) => write!(f, "{}: {m:?}", self.key()),
            Self::Network(n) => write!(f, "{}: {n:?}", self.key()),
            Self::HashMismatch(r) | Self::ReadBackMismatch(r) => {
                write!(f, "{}: {}", self.key(), r.name())
            }
            Self::RollbackRefused { floor, offered } => {
                write!(f, "{}: floor {floor}, offered {offered}", self.key())
            }
            Self::NoSpace { needed, free } => {
                write!(f, "{}: needed {needed}, free {free}", self.key())
            }
            _ => f.write_str(self.key()),
        }
    }
}

impl std::error::Error for UpdateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for UpdateError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<Malformed> for UpdateError {
    fn from(m: Malformed) -> Self {
        Self::Malformed(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_codes_round_trip() {
        let mut seen = std::collections::BTreeSet::new();
        for (index, key) in UpdateError::KEYS.iter().enumerate() {
            assert!(seen.insert(*key), "{key}");
            assert_eq!(UpdateError::key_of_code(index as u32), Some(*key));
        }
        assert_eq!(UpdateError::key_of_code(10_000), None);
        let e = UpdateError::RollbackRefused {
            floor: 3,
            offered: 2,
        };
        assert_eq!(UpdateError::key_of_code(e.code()), Some(e.key()));
        assert_eq!(UpdateError::Io(io::Error::other("x")).code(), 0);
        assert!(e.to_string().contains("floor 3"));
    }
}
