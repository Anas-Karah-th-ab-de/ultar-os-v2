//! Typed errors of the cloud client and their message keys.
//!
//! The server answers every failure with `{"error":"<code>"}`; [`CloudError::from_response`] maps
//! the code (and, for older servers, the bare status) to a variant. User-visible wording lives
//! only in the `locales/` catalogues; this module returns keys.

use std::fmt;

/// What the failing request was about; selects the wording for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    /// A store app (catalogue, details, package download, installation).
    App,
    /// A system update channel.
    UpdateChannel,
    /// Anything else (account, device, VPN).
    General,
}

/// Every error of the cloud client.
#[derive(Debug)]
pub enum CloudError {
    /// The server wants a signed-in account for this (server code `auth_required`).
    AuthRequired,
    /// Signed in, but the account's access does not include this (`not_entitled`).
    NotEntitled,
    /// The app, version, channel or release does not exist (`not_found`).
    NotFound,
    /// Too many requests (`rate_limited`).
    RateLimited {
        /// Seconds to wait before trying again.
        retry_after_secs: u64,
    },
    /// The server rejected the request as malformed (`invalid_request` and similar).
    InvalidRequest,
    /// Wrong username or password (`invalid_credentials`).
    InvalidCredentials,
    /// The username is already in use (`username_taken`).
    UsernameTaken,
    /// Any other error status.
    Server {
        /// HTTP status code.
        status: u16,
    },
    /// The server could not be reached (detail for logs, not for display).
    Transport(String),
    /// The server's answer could not be understood.
    InvalidResponse,
    /// A local file operation failed.
    Io(std::io::Error),
    /// A download did not match the announced size or SHA-256.
    Integrity,
    /// The package was refused on the device (missing, unknown or invalid signature …).
    Package(String),
    /// The operation needs an account and nobody is signed in on this device.
    NotSignedIn,
    /// Preparing the VPN keys failed (detail for logs).
    Vpn(String),
    /// The server refused the challenge (unknown, used, expired or issued for something else).
    ChallengeRejected,
    /// The server did not accept the device's signature (`signature_invalid`).
    SignatureRejected,
    /// The device key is registered to another account (`device_taken`).
    DeviceTaken,
    /// The device key was removed or replaced; the device needs a new identity (`device_retired`).
    DeviceRetired,
    /// The account has the allowed number of devices (`device_limit`, `device_quota_exhausted`).
    DeviceLimit,
    /// The server does not know this device (`device_unknown`).
    DeviceUnknown,
    /// The server needs a device key; this request had the form of the first release.
    UpdateRequired,
    /// The earlier registration cannot be taken over by this device (`legacy_not_eligible`).
    LegacyNotEligible,
    /// The waiting time of a device replacement is not over (`cooldown_active`).
    CooldownActive {
        /// Seconds left.
        retry_after_secs: u64,
    },
    /// This device was added without approval and may not manage other devices yet
    /// (`device_not_approved`).
    NotApproved {
        /// Seconds until it may without approval.
        retry_after_secs: u64,
    },
    /// The earlier registration cannot be replaced yet (`recovery_locked`).
    RecoveryLocked {
        /// Seconds until a replacement can be started.
        retry_after_secs: u64,
    },
    /// Another device replacement is already waiting (`recovery_pending`).
    RecoveryPending,
    /// There is no device replacement to finish or cancel (`recovery_unknown`).
    RecoveryUnknown,
    /// A challenge named another server or account than this device talks to; nothing was signed.
    ServerIdentityMismatch,
    /// The device key could not be used.
    KeyStore(crate::keystore::KeyStoreError),
    /// This device is not registered yet.
    NotRegistered,
}

impl CloudError {
    /// Maps an error response to a typed error. `code` is the `error` field of the body, if any.
    pub fn from_response(status: u16, code: Option<&str>, retry_after_secs: Option<u64>) -> Self {
        match (code, status) {
            (Some("auth_required"), _) => CloudError::AuthRequired,
            (Some("not_entitled"), _) => CloudError::NotEntitled,
            (Some("not_found"), _) => CloudError::NotFound,
            (Some("rate_limited"), _) | (None, 429) => CloudError::RateLimited {
                retry_after_secs: retry_after_secs.unwrap_or(1).max(1),
            },
            (Some("invalid_credentials"), _) => CloudError::InvalidCredentials,
            (Some("username_taken"), _) => CloudError::UsernameTaken,
            (Some("invalid_request" | "payload_too_large" | "invalid_wireguard_key"), _) => {
                CloudError::InvalidRequest
            }
            (Some("challenge_capacity"), _) => CloudError::RateLimited {
                retry_after_secs: retry_after_secs.unwrap_or(1).max(1),
            },
            (
                Some(
                    "challenge_unknown" | "challenge_used" | "challenge_expired"
                    | "challenge_mismatch",
                ),
                _,
            ) => CloudError::ChallengeRejected,
            (Some("signature_invalid"), _) => CloudError::SignatureRejected,
            (Some("device_taken"), _) => CloudError::DeviceTaken,
            (Some("device_retired"), _) => CloudError::DeviceRetired,
            (Some("device_limit" | "device_quota_exhausted"), _) => CloudError::DeviceLimit,
            (Some("device_unknown"), _) => CloudError::DeviceUnknown,
            (Some("device_key_required"), _) => CloudError::UpdateRequired,
            (Some("legacy_not_eligible"), _) => CloudError::LegacyNotEligible,
            (Some("cooldown_active"), _) => CloudError::CooldownActive {
                retry_after_secs: retry_after_secs.unwrap_or(1).max(1),
            },
            (Some("device_not_approved"), _) => CloudError::NotApproved {
                retry_after_secs: retry_after_secs.unwrap_or(1).max(1),
            },
            (Some("recovery_locked"), _) => CloudError::RecoveryLocked {
                retry_after_secs: retry_after_secs.unwrap_or(1).max(1),
            },
            (Some("recovery_pending"), _) => CloudError::RecoveryPending,
            (Some("recovery_unknown"), _) => CloudError::RecoveryUnknown,
            (None, 401) => CloudError::AuthRequired,
            (None, 403) => CloudError::NotEntitled,
            (None, 404) => CloudError::NotFound,
            (None, 400 | 413 | 422) => CloudError::InvalidRequest,
            _ => CloudError::Server { status },
        }
    }

    /// The message key for showing this error to the user in the given context.
    pub fn message_key(&self, subject: Subject) -> &'static str {
        match (self, subject) {
            (CloudError::AuthRequired, Subject::App) => "cloud.store.sign_in_required",
            (CloudError::AuthRequired, Subject::UpdateChannel) => "cloud.update.sign_in_required",
            (CloudError::AuthRequired, Subject::General) => "cloud.error.auth_required",
            (CloudError::NotEntitled, Subject::App) => "cloud.store.not_included",
            (CloudError::NotEntitled, Subject::UpdateChannel) => "cloud.update.not_included",
            (CloudError::NotEntitled, Subject::General) => "cloud.error.not_entitled",
            (CloudError::NotFound, Subject::App) => "cloud.store.not_found",
            (CloudError::NotFound, Subject::UpdateChannel) => "cloud.update.not_found",
            (CloudError::NotFound, Subject::General) => "cloud.error.not_found",
            (CloudError::RateLimited { .. }, _) => "cloud.error.rate_limited",
            (CloudError::InvalidRequest, _) => "cloud.error.invalid_request",
            (CloudError::InvalidCredentials, _) => "cloud.error.invalid_credentials",
            (CloudError::UsernameTaken, _) => "cloud.error.username_taken",
            (CloudError::Server { .. }, _) => "cloud.error.server",
            (CloudError::Transport(_), _) => "cloud.error.offline",
            (CloudError::InvalidResponse, _) => "cloud.error.invalid_response",
            (CloudError::Io(_), _) => "cloud.error.io",
            (CloudError::Integrity, _) => "cloud.error.integrity",
            (CloudError::Package(_), _) => "cloud.error.package_rejected",
            (CloudError::NotSignedIn, _) => "cloud.error.not_signed_in",
            (CloudError::Vpn(_), _) => "cloud.error.vpn",
            (CloudError::ChallengeRejected, _) => "cloud.device.challenge_rejected",
            (CloudError::SignatureRejected, _) => "cloud.device.signature_rejected",
            (CloudError::DeviceTaken, _) => "cloud.device.taken",
            (CloudError::DeviceRetired, _) => "cloud.device.retired",
            (CloudError::DeviceLimit, _) => "cloud.device.limit",
            (CloudError::DeviceUnknown, _) => "cloud.device.unknown",
            (CloudError::UpdateRequired, _) => "cloud.device.update_required",
            (CloudError::LegacyNotEligible, _) => "cloud.device.legacy_not_eligible",
            (CloudError::CooldownActive { .. }, _) => "cloud.device.cooldown_active",
            (CloudError::NotApproved { .. }, _) => "cloud.device.not_approved",
            (CloudError::RecoveryLocked { .. }, _) => "cloud.device.recovery_locked",
            (CloudError::RecoveryPending, _) => "cloud.device.recovery_pending",
            (CloudError::RecoveryUnknown, _) => "cloud.device.recovery_unknown",
            (CloudError::ServerIdentityMismatch, _) => "cloud.device.server_mismatch",
            (CloudError::KeyStore(_), _) => "cloud.device.key_unusable",
            (CloudError::NotRegistered, _) => "cloud.device.not_registered",
        }
    }

    /// Seconds to wait, for the `{seconds}` placeholder of `cloud.error.rate_limited` and
    /// `cloud.device.cooldown_active`.
    pub fn retry_after_secs(&self) -> Option<u64> {
        match self {
            CloudError::RateLimited { retry_after_secs }
            | CloudError::NotApproved { retry_after_secs }
            | CloudError::RecoveryLocked { retry_after_secs }
            | CloudError::CooldownActive { retry_after_secs } => Some(*retry_after_secs),
            _ => None,
        }
    }
}

impl fmt::Display for CloudError {
    /// Technical description for logs (English, not for the user interface).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CloudError::RateLimited { retry_after_secs } => {
                write!(f, "rate limited, retry after {retry_after_secs} s")
            }
            CloudError::Server { status } => write!(f, "server error, status {status}"),
            CloudError::Transport(detail) => write!(f, "transport: {detail}"),
            CloudError::Io(e) => write!(f, "i/o: {e}"),
            CloudError::Package(detail) => write!(f, "package rejected: {detail}"),
            CloudError::Vpn(detail) => write!(f, "vpn: {detail}"),
            CloudError::CooldownActive { retry_after_secs } => {
                write!(f, "cool-down active, {retry_after_secs} s left")
            }
            CloudError::NotApproved { retry_after_secs } => {
                write!(f, "device not approved, {retry_after_secs} s left")
            }
            CloudError::RecoveryLocked { retry_after_secs } => {
                write!(f, "recovery locked, {retry_after_secs} s left")
            }
            CloudError::KeyStore(e) => write!(f, "device key: {e}"),
            other => f.write_str(other.message_key(Subject::General)),
        }
    }
}

impl std::error::Error for CloudError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CloudError::Io(e) => Some(e),
            CloudError::KeyStore(e) => Some(e),
            _ => None,
        }
    }
}

impl From<crate::keystore::KeyStoreError> for CloudError {
    fn from(e: crate::keystore::KeyStoreError) -> Self {
        CloudError::KeyStore(e)
    }
}

impl From<std::io::Error> for CloudError {
    fn from(e: std::io::Error) -> Self {
        CloudError::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_codes_map_to_variants() {
        let f = CloudError::from_response;
        assert!(matches!(
            f(401, Some("auth_required"), None),
            CloudError::AuthRequired
        ));
        assert!(matches!(
            f(403, Some("not_entitled"), None),
            CloudError::NotEntitled
        ));
        assert!(matches!(
            f(404, Some("not_found"), None),
            CloudError::NotFound
        ));
        assert!(matches!(
            f(429, Some("rate_limited"), Some(7)),
            CloudError::RateLimited {
                retry_after_secs: 7
            }
        ));
        assert!(matches!(
            f(429, None, None),
            CloudError::RateLimited {
                retry_after_secs: 1
            }
        ));
        assert!(matches!(
            f(401, Some("invalid_credentials"), None),
            CloudError::InvalidCredentials
        ));
        assert!(matches!(
            f(409, Some("username_taken"), None),
            CloudError::UsernameTaken
        ));
        assert!(matches!(
            f(400, Some("invalid_request"), None),
            CloudError::InvalidRequest
        ));
        assert!(matches!(
            f(413, Some("payload_too_large"), None),
            CloudError::InvalidRequest
        ));
        assert!(matches!(
            f(500, Some("internal"), None),
            CloudError::Server { status: 500 }
        ));
        assert!(matches!(
            f(503, Some("something_new"), None),
            CloudError::Server { status: 503 }
        ));
    }

    #[test]
    fn bare_statuses_of_older_servers_map_too() {
        let f = CloudError::from_response;
        assert!(matches!(f(401, None, None), CloudError::AuthRequired));
        assert!(matches!(f(403, None, None), CloudError::NotEntitled));
        assert!(matches!(f(404, None, None), CloudError::NotFound));
        assert!(matches!(f(422, None, None), CloudError::InvalidRequest));
        assert!(matches!(
            f(502, None, None),
            CloudError::Server { status: 502 }
        ));
    }

    #[test]
    fn keys_depend_on_the_subject_for_access_errors() {
        let e = CloudError::AuthRequired;
        assert_eq!(e.message_key(Subject::App), "cloud.store.sign_in_required");
        assert_eq!(
            e.message_key(Subject::UpdateChannel),
            "cloud.update.sign_in_required"
        );
        assert_eq!(e.message_key(Subject::General), "cloud.error.auth_required");
        let e = CloudError::NotEntitled;
        assert_eq!(e.message_key(Subject::App), "cloud.store.not_included");
        assert_eq!(
            e.message_key(Subject::UpdateChannel),
            "cloud.update.not_included"
        );
        assert_eq!(
            CloudError::Integrity.message_key(Subject::App),
            "cloud.error.integrity"
        );
    }

    #[test]
    fn display_and_source() {
        let io = CloudError::from(std::io::Error::other("disk"));
        assert!(io.to_string().contains("disk"));
        assert!(std::error::Error::source(&io).is_some());
        assert!(std::error::Error::source(&CloudError::NotFound).is_none());
        assert_eq!(CloudError::NotFound.to_string(), "cloud.error.not_found");
        assert_eq!(
            CloudError::RateLimited {
                retry_after_secs: 3
            }
            .retry_after_secs(),
            Some(3)
        );
        assert_eq!(CloudError::NotFound.retry_after_secs(), None);
        assert!(CloudError::Server { status: 500 }
            .to_string()
            .contains("500"));
        assert!(CloudError::Transport("refused".into())
            .to_string()
            .contains("refused"));
        assert!(CloudError::Package("sig".into())
            .to_string()
            .contains("sig"));
        assert!(CloudError::Vpn("keys".into()).to_string().contains("keys"));
    }

    #[test]
    fn device_codes_map_to_variants() {
        let f = CloudError::from_response;
        for code in [
            "challenge_unknown",
            "challenge_used",
            "challenge_expired",
            "challenge_mismatch",
        ] {
            assert!(matches!(
                f(400, Some(code), None),
                CloudError::ChallengeRejected
            ));
        }
        assert!(matches!(
            f(429, Some("challenge_capacity"), Some(120)),
            CloudError::RateLimited {
                retry_after_secs: 120
            }
        ));
        assert!(matches!(
            f(401, Some("signature_invalid"), None),
            CloudError::SignatureRejected
        ));
        assert!(matches!(
            f(409, Some("device_taken"), None),
            CloudError::DeviceTaken
        ));
        assert!(matches!(
            f(410, Some("device_retired"), None),
            CloudError::DeviceRetired
        ));
        assert!(matches!(
            f(409, Some("device_limit"), None),
            CloudError::DeviceLimit
        ));
        assert!(matches!(
            f(409, Some("device_quota_exhausted"), None),
            CloudError::DeviceLimit
        ));
        assert!(matches!(
            f(404, Some("device_unknown"), None),
            CloudError::DeviceUnknown
        ));
    }

    #[test]
    fn upgrade_recovery_and_key_errors_map_to_variants() {
        let f = CloudError::from_response;
        assert!(matches!(
            f(426, Some("device_key_required"), None),
            CloudError::UpdateRequired
        ));
        assert!(matches!(
            f(403, Some("legacy_not_eligible"), None),
            CloudError::LegacyNotEligible
        ));
        let wait = f(409, Some("cooldown_active"), Some(3600));
        assert_eq!(wait.retry_after_secs(), Some(3600));
        assert!(wait.to_string().contains("3600"));
        assert!(matches!(
            f(409, Some("recovery_pending"), None),
            CloudError::RecoveryPending
        ));
        assert!(matches!(
            f(404, Some("recovery_unknown"), None),
            CloudError::RecoveryUnknown
        ));
        let wait = f(403, Some("device_not_approved"), Some(120));
        assert!(matches!(
            wait,
            CloudError::NotApproved {
                retry_after_secs: 120
            }
        ));
        assert_eq!(wait.retry_after_secs(), Some(120));
        assert!(wait.to_string().contains("120"));
        let locked = f(409, Some("recovery_locked"), None);
        assert_eq!(locked.retry_after_secs(), Some(1));
        assert!(locked.to_string().contains("locked"));
        let key = CloudError::from(crate::keystore::KeyStoreError::Corrupt);
        assert!(std::error::Error::source(&key).is_some());
        assert!(key.to_string().contains("damaged"));
        assert_eq!(
            key.message_key(Subject::General),
            "cloud.device.key_unusable"
        );
    }
}
