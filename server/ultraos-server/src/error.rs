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
    /// The device id is already registered to another account.
    DeviceTaken,
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
            ApiError::VpnPoolExhausted => "vpn_pool_exhausted",
            ApiError::Internal => "internal",
        }
    }

    /// The HTTP status that goes with the code.
    pub fn status(&self) -> StatusCode {
        match self {
            ApiError::AuthRequired | ApiError::InvalidCredentials => StatusCode::UNAUTHORIZED,
            ApiError::NotEntitled => StatusCode::FORBIDDEN,
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            ApiError::InvalidRequest | ApiError::InvalidWireguardKey => StatusCode::BAD_REQUEST,
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

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({ "error": self.code() }));
        let mut resp = (self.status(), body).into_response();
        let headers = resp.headers_mut();
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        match self {
            ApiError::RateLimited { retry_after_secs } => {
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

    const ALL: [ApiError; 14] = [
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
}
