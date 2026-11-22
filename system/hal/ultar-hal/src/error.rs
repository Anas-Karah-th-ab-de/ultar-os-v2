//! Error type shared by every HAL trait.

use std::fmt;

/// Result of a HAL call.
pub type HalResult<T> = Result<T, HalError>;

/// What can go wrong when talking to hardware.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HalError {
    /// The hardware or backend cannot do this at all.
    NotSupported,
    /// The addressed device, mode, endpoint or stream does not exist.
    NotFound,
    /// A parameter is outside the range the device accepts.
    InvalidArgument,
    /// The device is in use or still working on an earlier request.
    Busy,
    /// The device did not answer in time.
    Timeout,
    /// The caller is not allowed to do this.
    PermissionDenied,
    /// The device was removed or switched off.
    Disconnected,
    /// The request was understood but refused (wrong password, no SIM, radio off …).
    Rejected,
    /// The operating system reported an I/O error; carries the error number.
    Io(i32),
}

impl HalError {
    /// Stable message key for translation (see [`crate::msg`]).
    pub const fn key(self) -> &'static str {
        match self {
            HalError::NotSupported => "hal.error.not_supported",
            HalError::NotFound => "hal.error.not_found",
            HalError::InvalidArgument => "hal.error.invalid_argument",
            HalError::Busy => "hal.error.busy",
            HalError::Timeout => "hal.error.timeout",
            HalError::PermissionDenied => "hal.error.permission_denied",
            HalError::Disconnected => "hal.error.disconnected",
            HalError::Rejected => "hal.error.rejected",
            HalError::Io(_) => "hal.error.io",
        }
    }

    /// True if trying the same call again later can succeed.
    pub const fn is_transient(self) -> bool {
        matches!(self, HalError::Busy | HalError::Timeout)
    }
}

impl fmt::Display for HalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HalError::Io(code) => write!(f, "{} ({code})", self.key()),
            other => f.write_str(other.key()),
        }
    }
}

impl std::error::Error for HalError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::{self, Lang};

    const ALL: [HalError; 9] = [
        HalError::NotSupported,
        HalError::NotFound,
        HalError::InvalidArgument,
        HalError::Busy,
        HalError::Timeout,
        HalError::PermissionDenied,
        HalError::Disconnected,
        HalError::Rejected,
        HalError::Io(5),
    ];

    #[test]
    fn every_error_has_key_and_text() {
        for error in ALL {
            assert!(error.to_string().starts_with(error.key()));
            for lang in Lang::ALL {
                assert!(msg::text(error.key(), lang).is_some(), "{error:?}");
            }
        }
        assert_eq!(HalError::Io(5).to_string(), "hal.error.io (5)");
    }

    #[test]
    fn transient_errors() {
        let transient: Vec<HalError> = ALL.into_iter().filter(|e| e.is_transient()).collect();
        assert_eq!(transient, vec![HalError::Busy, HalError::Timeout]);
    }

    #[test]
    fn is_std_error() {
        let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(HalError::Busy);
        assert_eq!(boxed.to_string(), "hal.error.busy");
    }
}
