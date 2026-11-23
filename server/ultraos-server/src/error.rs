//! Typed API errors. Every error leaves the server as `{"error":"<code>"}` with a fixed status.
//!
//! The code is a stable machine-readable key; the device maps it to a translated text.
//! No free-form or user-language text is ever produced here.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use std::fmt;

/// Every error an endpoint can return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiError {
    /// A valid session token is needed and was missing, malformed, forged or expired.
    AuthRequired,
    /// The token is valid, but the account has no entitlement for the resource.
    NotEntitled,
    /// The resource does not exist (also used for malformed identifiers).
    NotFound,
    /// The caller exceeded the rate limit; retry after the given number of seconds.
    RateLimited {
        /// Seconds until one request is allowed again.
        retry_after_secs: u64,
    },
    /// A parameter, header or body was malformed.
    InvalidRequest,
    /// The request body or URI exceeded the size limit.
    PayloadTooLarge,
    /// The requested byte range lies outside the file.
    RangeNotSatisfiable {
        /// Total size of the file in bytes.
        size: u64,
    },
    /// Registration: the username is already in use.
    UsernameTaken,
    /// Login: unknown username or wrong password.
    InvalidCredentials,
    /// VPN: the submitted WireGuard public key is not a 32-byte base64 key.
    InvalidWireguardKey,
    /// TURN is not configured on this server.
    TurnNotConfigured,
    /// The device key is already registered to another account.
    DeviceTaken,
    /// No challenge with this id (never issued, or forgotten).
    ChallengeUnknown,
    /// The challenge was presented before (replay).
    ChallengeUsed,
    /// The challenge is older than its lifetime.
    ChallengeExpired,
    /// The challenge was issued to another account or for another purpose.
    ChallengeMismatch,
    /// Too many challenges are pending; retry after the given number of seconds.
    ChallengeCapacity {
        /// Seconds until a new challenge can be issued.
        retry_after_secs: u64,
    },
    /// A signature did not verify for the claimed key and this request.
    SignatureInvalid,
    /// No such device in this account.
    DeviceUnknown,
    /// The device key was removed or replaced and stays retired.
    DeviceRetired,
    /// The account already has the allowed number of devices.
    DeviceLimit,
    /// The account used up its lifetime number of device registrations.
    DeviceQuotaExhausted,
    /// The request has the form of the first server release; a device key is needed now.
    DeviceKeyRequired,
    /// The registration from before device keys cannot be upgraded by this caller.
    LegacyNotEligible,
    /// The cool-down of a recovery is not over; retry after the given number of seconds.
    CooldownActive {
        /// Seconds left.
        retry_after_secs: u64,
    },
    /// Another recovery is pending for the account.
    RecoveryPending,
    /// There is no matching recovery.
    RecoveryUnknown,
    /// VPN: the internal address pool is used up.
    VpnPoolExhausted,
    /// Unexpected server-side failure (details are only in the server log).
    Internal,
}

impl ApiError {
    /// The stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            ApiError::AuthRequired => "auth_required",
            ApiError::NotEntitled => "not_entitled",
            ApiError::NotFound => "not_found",
            ApiError::RateLimited { .. } => "rate_limited",
            ApiError::InvalidRequest => "invalid_request",
            ApiError::PayloadTooLarge => "payload_too_large",
            ApiError::RangeNotSatisfiable { .. } => "range_not_satisfiable",
            ApiError::UsernameTaken => "username_taken",
            ApiError::InvalidCredentials => "invalid_credentials",
            ApiError::InvalidWireguardKey => "invalid_wireguard_key",
            ApiError::TurnNotConfigured => "turn_not_configured",
            ApiError::DeviceTaken => "device_taken",
            ApiError::ChallengeUnknown => "challenge_unknown",
            ApiError::ChallengeUsed => "challenge_used",
            ApiError::ChallengeExpired => "challenge_expired",
            ApiError::ChallengeMismatch => "challenge_mismatch",
            ApiError::ChallengeCapacity { .. } => "challenge_capacity",
            ApiError::SignatureInvalid => "signature_invalid",
            ApiError::DeviceUnknown => "device_unknown",
            ApiError::DeviceRetired => "device_retired",
            ApiError::DeviceLimit => "device_limit",
            ApiError::DeviceQuotaExhausted => "device_quota_exhausted",
            ApiError::DeviceKeyRequired => "device_key_required",
            ApiError::LegacyNotEligible => "legacy_not_eligible",
            ApiError::CooldownActive { .. } => "cooldown_active",
            ApiError::RecoveryPending => "recovery_pending",
            ApiError::RecoveryUnknown => "recovery_unknown",
            ApiError::VpnPoolExhausted => "vpn_pool_exhausted",
            ApiError::Internal => "internal",
        }
    }

    /// The HTTP status that goes with the code.
    pub fn status(&self) -> StatusCode {
        match self {
            ApiError::AuthRequired | ApiError::InvalidCredentials | ApiError::SignatureInvalid => {
                StatusCode::UNAUTHORIZED
            }
            ApiError::NotEntitled | ApiError::LegacyNotEligible => StatusCode::FORBIDDEN,
            ApiError::NotFound | ApiError::DeviceUnknown | ApiError::RecoveryUnknown => {
                StatusCode::NOT_FOUND
            }
            ApiError::RateLimited { .. } | ApiError::ChallengeCapacity { .. } => {
                StatusCode::TOO_MANY_REQUESTS
            }
            ApiError::InvalidRequest
            | ApiError::InvalidWireguardKey
            | ApiError::ChallengeUnknown
            | ApiError::ChallengeUsed
            | ApiError::ChallengeExpired
            | ApiError::ChallengeMismatch => StatusCode::BAD_REQUEST,
            ApiError::DeviceRetired => StatusCode::GONE,
            ApiError::DeviceKeyRequired => StatusCode::UPGRADE_REQUIRED,
            ApiError::DeviceLimit
            | ApiError::DeviceQuotaExhausted
            | ApiError::CooldownActive { .. }
            | ApiError::RecoveryPending => StatusCode::CONFLICT,
            ApiError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ApiError::RangeNotSatisfiable { .. } => StatusCode::RANGE_NOT_SATISFIABLE,
            ApiError::UsernameTaken | ApiError::DeviceTaken => StatusCode::CONFLICT,
            ApiError::TurnNotConfigured | ApiError::VpnPoolExhausted => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for ApiError {}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        tracing::error!(event = "db_error", error = %e);
        ApiError::Internal
    }
}

impl From<crate::challenge::ChallengeError> for ApiError {
    fn from(e: crate::challenge::ChallengeError) -> Self {
        use crate::challenge::ChallengeError as C;
        match e {
            C::Unknown => ApiError::ChallengeUnknown,
            C::Used => ApiError::ChallengeUsed,
            C::Expired => ApiError::ChallengeExpired,
            C::Mismatch => ApiError::ChallengeMismatch,
            C::Capacity { retry_after_secs } => ApiError::ChallengeCapacity { retry_after_secs },
        }
    }
}

impl From<crate::identity::PopError> for ApiError {
    fn from(e: crate::identity::PopError) -> Self {
        use crate::identity::PopError as P;
        match e {
            P::MalformedKey | P::WeakKey | P::MalformedSignature => ApiError::InvalidRequest,
            P::BadSignature => ApiError::SignatureInvalid,
        }
    }
}

impl From<crate::devices::DeviceError> for ApiError {
    fn from(e: crate::devices::DeviceError) -> Self {
        use crate::devices::DeviceError as D;
        match e {
            D::Taken => ApiError::DeviceTaken,
            D::Retired => ApiError::DeviceRetired,
            D::Limit => ApiError::DeviceLimit,
            D::QuotaExhausted => ApiError::DeviceQuotaExhausted,
            D::Unknown => ApiError::DeviceUnknown,
            D::LegacyNotEligible => ApiError::LegacyNotEligible,
            D::InvalidName => ApiError::InvalidRequest,
            D::RecoveryPending => ApiError::RecoveryPending,
            D::RecoveryUnknown => ApiError::RecoveryUnknown,
            D::CooldownActive { retry_after_secs } => ApiError::CooldownActive { retry_after_secs },
            D::NumbersExhausted => {
                tracing::error!(event = "numbers_exhausted");
                ApiError::Internal
            }
            D::Db(e) => e.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({ "error": self.code() }));
        let mut resp = (self.status(), body).into_response();
        let headers = resp.headers_mut();
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        match self {
            ApiError::RateLimited { retry_after_secs }
            | ApiError::ChallengeCapacity { retry_after_secs }
            | ApiError::CooldownActive { retry_after_secs } => {
                headers.insert(header::RETRY_AFTER, HeaderValue::from(retry_after_secs));
            }
            ApiError::RangeNotSatisfiable { size } => {
                if let Ok(v) = HeaderValue::from_str(&format!("bytes */{size}")) {
                    headers.insert(header::CONTENT_RANGE, v);
                }
            }
            ApiError::AuthRequired => {
                headers.insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
            }
            _ => {}
        }
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [ApiError; 29] = [
        ApiError::ChallengeUnknown,
        ApiError::ChallengeUsed,
        ApiError::ChallengeExpired,
        ApiError::ChallengeMismatch,
        ApiError::ChallengeCapacity {
            retry_after_secs: 3,
        },
        ApiError::SignatureInvalid,
        ApiError::DeviceUnknown,
        ApiError::DeviceRetired,
        ApiError::DeviceLimit,
        ApiError::DeviceQuotaExhausted,
        ApiError::DeviceKeyRequired,
        ApiError::LegacyNotEligible,
        ApiError::CooldownActive {
            retry_after_secs: 9,
        },
        ApiError::RecoveryPending,
        ApiError::RecoveryUnknown,
        ApiError::DeviceTaken,
        ApiError::AuthRequired,
        ApiError::NotEntitled,
        ApiError::NotFound,
        ApiError::RateLimited {
            retry_after_secs: 7,
        },
        ApiError::InvalidRequest,
        ApiError::PayloadTooLarge,
        ApiError::RangeNotSatisfiable { size: 10 },
        ApiError::UsernameTaken,
        ApiError::InvalidCredentials,
        ApiError::InvalidWireguardKey,
        ApiError::TurnNotConfigured,
        ApiError::VpnPoolExhausted,
        ApiError::Internal,
    ];

    #[test]
    fn codes_are_unique_snake_case_and_statuses_are_errors() {
        let mut seen = std::collections::HashSet::new();
        for e in ALL {
            assert!(seen.insert(e.code()), "duplicate code {}", e.code());
            assert!(e
                .code()
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'_'));
            assert!(e.status().is_client_error() || e.status().is_server_error());
            assert_eq!(e.to_string(), e.code());
        }
    }

    #[test]
    fn required_codes_have_the_documented_statuses() {
        assert_eq!(ApiError::AuthRequired.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(ApiError::NotEntitled.status(), StatusCode::FORBIDDEN);
        assert_eq!(ApiError::NotFound.status(), StatusCode::NOT_FOUND);
        let limited = ApiError::RateLimited {
            retry_after_secs: 1,
        };
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn responses_carry_the_special_headers() {
        let r = ApiError::RateLimited {
            retry_after_secs: 7,
        }
        .into_response();
        assert_eq!(
            r.headers()
                .get(header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok()),
            Some("7")
        );
        let r = ApiError::RangeNotSatisfiable { size: 10 }.into_response();
        let cr = r
            .headers()
            .get(header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok());
        assert_eq!(cr, Some("bytes */10"));
        let r = ApiError::AuthRequired.into_response();
        assert!(r.headers().contains_key(header::WWW_AUTHENTICATE));
    }

    #[test]
    fn database_errors_become_internal() {
        let e: ApiError = rusqlite::Error::QueryReturnedNoRows.into();
        assert_eq!(e, ApiError::Internal);
    }

    #[test]
    fn device_errors_map_to_their_codes() {
        use crate::challenge::ChallengeError as C;
        use crate::devices::DeviceError as D;
        use crate::identity::PopError as P;
        let code = |e: ApiError| e.code();
        assert_eq!(code(C::Unknown.into()), "challenge_unknown");
        assert_eq!(code(C::Used.into()), "challenge_used");
        assert_eq!(code(C::Expired.into()), "challenge_expired");
        assert_eq!(code(C::Mismatch.into()), "challenge_mismatch");
        let full: ApiError = C::Capacity {
            retry_after_secs: 4,
        }
        .into();
        assert_eq!(full.status(), StatusCode::TOO_MANY_REQUESTS);
        let r = full.into_response();
        assert_eq!(
            r.headers()
                .get(header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok()),
            Some("4")
        );
        assert_eq!(code(P::BadSignature.into()), "signature_invalid");
        for malformed in [P::MalformedKey, P::WeakKey, P::MalformedSignature] {
            assert_eq!(code(malformed.into()), "invalid_request");
        }
        for (error, expected) in [
            (D::Taken, "device_taken"),
            (D::Retired, "device_retired"),
            (D::Limit, "device_limit"),
            (D::QuotaExhausted, "device_quota_exhausted"),
            (D::Unknown, "device_unknown"),
            (D::LegacyNotEligible, "legacy_not_eligible"),
            (D::InvalidName, "invalid_request"),
            (D::RecoveryPending, "recovery_pending"),
            (D::RecoveryUnknown, "recovery_unknown"),
            (D::NumbersExhausted, "internal"),
            (D::Db(rusqlite::Error::QueryReturnedNoRows), "internal"),
        ] {
            assert_eq!(code(error.into()), expected);
        }
        let wait: ApiError = D::CooldownActive {
            retry_after_secs: 60,
        }
        .into();
        assert_eq!(
            (wait.code(), wait.status()),
            ("cooldown_active", StatusCode::CONFLICT)
        );
        assert_eq!(ApiError::DeviceRetired.status(), StatusCode::GONE);
        assert_eq!(
            ApiError::DeviceKeyRequired.status(),
            StatusCode::UPGRADE_REQUIRED
        );
    }
}
