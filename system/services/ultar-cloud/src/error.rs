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
        }
    }

    /// Seconds to wait, for the `{seconds}` placeholder of `cloud.error.rate_limited`.
    pub fn retry_after_secs(&self) -> Option<u64> {
        match self {
            CloudError::RateLimited { retry_after_secs } => Some(*retry_after_secs),
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
            other => f.write_str(other.message_key(Subject::General)),
        }
    }
}

impl std::error::Error for CloudError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CloudError::Io(e) => Some(e),
            _ => None,
        }
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
}
