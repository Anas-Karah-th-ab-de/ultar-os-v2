//! The typed failures of the settings service.

use crate::apply::ApplyError;
use crate::schema::{LookupError, ValueError};
use crate::store::StoreError;
use std::fmt;
use ultar_bus::messages::settings::code;
use ultar_bus::ErrorCode;

/// Why a request to the settings service failed.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsError {
    /// The key is not syntactically valid.
    InvalidKey,
    /// No setting is declared under the key.
    UnknownKey,
    /// The key names a display or app that is not known.
    UnknownInstance,
    /// The value is not acceptable for the setting.
    Value(ValueError),
    /// The device lacks a capability the setting needs.
    Unavailable,
    /// The caller may not change this setting.
    NotPermitted,
    /// Nothing is waiting for confirmation under the key.
    NotPending,
    /// A limit of the service was reached.
    Limit,
    /// The component that puts the value into effect refused it.
    Apply(ApplyError),
    /// The value could not be written to disk.
    Store(StoreError),
    /// A request field is not acceptable.
    BadRequest,
}

impl SettingsError {
    /// The code sent over the bus.
    pub const fn code(&self) -> ErrorCode {
        let service = match self {
            SettingsError::NotPermitted => return ErrorCode::PermissionDenied,
            SettingsError::InvalidKey | SettingsError::UnknownKey => code::UNKNOWN_KEY,
            SettingsError::UnknownInstance => code::UNKNOWN_INSTANCE,
            SettingsError::Value(ValueError::TypeMismatch { .. }) => code::TYPE_MISMATCH,
            SettingsError::Value(ValueError::OutOfRange | ValueError::OffStep) => {
                code::OUT_OF_RANGE
            }
            SettingsError::Value(_) => code::INVALID_VALUE,
            SettingsError::Unavailable => code::UNAVAILABLE,
            SettingsError::NotPending => code::NOT_PENDING,
            SettingsError::Limit => code::LIMIT,
            SettingsError::Apply(_) => code::APPLY_FAILED,
            SettingsError::Store(_) => code::STORE_FAILED,
            SettingsError::BadRequest => code::BAD_REQUEST,
        };
        ErrorCode::Service(service)
    }

    /// Key of the user-facing message.
    pub const fn message_key(&self) -> &'static str {
        message_key_of(self.code())
    }
}

/// Key of the user-facing message for an error code received over the bus.
pub const fn message_key_of(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::PermissionDenied => "settingsd.error.not_permitted",
        ErrorCode::Service(code::UNKNOWN_KEY) => "settingsd.error.unknown_key",
        ErrorCode::Service(code::UNKNOWN_INSTANCE) => "settingsd.error.unknown_instance",
        ErrorCode::Service(code::TYPE_MISMATCH) => "settingsd.error.type_mismatch",
        ErrorCode::Service(code::OUT_OF_RANGE) => "settingsd.error.out_of_range",
        ErrorCode::Service(code::INVALID_VALUE) => "settingsd.error.invalid_value",
        ErrorCode::Service(code::UNAVAILABLE) => "settingsd.error.unavailable",
        ErrorCode::Service(code::NOT_PENDING) => "settingsd.error.not_pending",
        ErrorCode::Service(code::LIMIT) => "settingsd.error.limit",
        ErrorCode::Service(code::APPLY_FAILED) => "settingsd.error.apply_failed",
        ErrorCode::Service(code::STORE_FAILED) => "settingsd.error.store_failed",
        ErrorCode::Service(code::BAD_REQUEST) => "settingsd.error.bad_request",
        ErrorCode::UnknownService | ErrorCode::ServiceGone => "settingsd.error.no_service",
        _ => "settingsd.error.bus",
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingsError::InvalidKey => f.write_str("invalid setting key"),
            SettingsError::UnknownKey => f.write_str("unknown setting"),
            SettingsError::UnknownInstance => f.write_str("unknown display or app"),
            SettingsError::Value(error) => write!(f, "invalid value: {error}"),
            SettingsError::Unavailable => f.write_str("setting not available on this device"),
            SettingsError::NotPermitted => f.write_str("not permitted"),
            SettingsError::NotPending => f.write_str("no change is waiting for confirmation"),
            SettingsError::Limit => f.write_str("limit reached"),
            SettingsError::Apply(error) => write!(f, "could not apply: {error}"),
            SettingsError::Store(error) => write!(f, "could not store: {error}"),
            SettingsError::BadRequest => f.write_str("bad request"),
        }
    }
}

impl std::error::Error for SettingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SettingsError::Value(error) => Some(error),
            SettingsError::Apply(error) => Some(error),
            SettingsError::Store(error) => Some(error),
            _ => None,
        }
    }
}

impl From<LookupError> for SettingsError {
    fn from(error: LookupError) -> Self {
        match error {
            LookupError::InvalidKey => SettingsError::InvalidKey,
            LookupError::UnknownKey => SettingsError::UnknownKey,
        }
    }
}

impl From<ValueError> for SettingsError {
    fn from(error: ValueError) -> Self {
        SettingsError::Value(error)
    }
}

impl From<ApplyError> for SettingsError {
    fn from(error: ApplyError) -> Self {
        SettingsError::Apply(error)
    }
}

impl From<StoreError> for SettingsError {
    fn from(error: StoreError) -> Self {
        SettingsError::Store(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    fn all() -> Vec<SettingsError> {
        vec![
            SettingsError::InvalidKey,
            SettingsError::UnknownKey,
            SettingsError::UnknownInstance,
            SettingsError::Value(ValueError::TypeMismatch {
                expected: "bool",
                found: "int",
            }),
            SettingsError::Value(ValueError::OutOfRange),
            SettingsError::Value(ValueError::UnknownChoice),
            SettingsError::Unavailable,
            SettingsError::NotPermitted,
            SettingsError::NotPending,
            SettingsError::Limit,
            SettingsError::Apply(ApplyError::Busy),
            SettingsError::Store(StoreError::InvalidUser),
            SettingsError::BadRequest,
        ]
    }

    #[test]
    fn every_error_has_a_code_a_key_and_text() {
        let mut codes = Vec::new();
        for error in all() {
            assert!(error.message_key().starts_with("settingsd.error."));
            assert_ne!(error.message_key(), "settingsd.error.bus");
            assert!(!error.to_string().is_empty());
            codes.push(error.code());
        }
        codes.dedup();
        // InvalidKey and UnknownKey share a code on purpose.
        assert_eq!(codes.len(), all().len() - 1);
        assert_eq!(
            SettingsError::NotPermitted.code(),
            ErrorCode::PermissionDenied
        );
        assert_eq!(
            message_key_of(ErrorCode::UnknownService),
            "settingsd.error.no_service"
        );
        assert_eq!(
            message_key_of(ErrorCode::Service(999)),
            "settingsd.error.bus"
        );
    }

    #[test]
    fn conversions_and_sources() {
        assert_eq!(
            SettingsError::from(LookupError::InvalidKey),
            SettingsError::InvalidKey
        );
        assert_eq!(
            SettingsError::from(LookupError::UnknownKey),
            SettingsError::UnknownKey
        );
        assert!(SettingsError::from(ValueError::TooLong).source().is_some());
        assert!(SettingsError::from(ApplyError::Failed).source().is_some());
        assert!(SettingsError::from(StoreError::InvalidUser)
            .source()
            .is_some());
        assert!(SettingsError::Limit.source().is_none());
    }
}
