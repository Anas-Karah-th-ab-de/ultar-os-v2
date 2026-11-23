//! Error type of the graphics backends.

use std::fmt;
use ultar_hal::HalError;

/// What went wrong in a graphics backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GfxError {
    /// The kernel refused a call; carries the error number.
    Sys(i32),
    /// A size, offset or format is outside what the call accepts.
    InvalidArgument,
    /// A shared buffer is not sealed against shrinking (it could be cut
    /// while the compositor reads it).
    NotSealed,
    /// A shared buffer is smaller than the pixels it must hold.
    TooSmall {
        /// Bytes needed.
        needed: u64,
        /// Bytes the buffer has.
        actual: u64,
    },
    /// A shared buffer is larger than [`crate::shm::MAX_SHARED_BYTES`].
    TooLarge,
    /// No usable device was found.
    NoDevice,
    /// The device works with a pixel layout this backend does not handle.
    UnsupportedFormat,
}

impl GfxError {
    /// Stable, language independent key of the error.
    pub const fn key(self) -> &'static str {
        match self {
            GfxError::Sys(_) => "gfx.error.sys",
            GfxError::InvalidArgument => "gfx.error.invalid_argument",
            GfxError::NotSealed => "gfx.error.not_sealed",
            GfxError::TooSmall { .. } => "gfx.error.too_small",
            GfxError::TooLarge => "gfx.error.too_large",
            GfxError::NoDevice => "gfx.error.no_device",
            GfxError::UnsupportedFormat => "gfx.error.unsupported_format",
        }
    }
}

impl From<rustix::io::Errno> for GfxError {
    fn from(errno: rustix::io::Errno) -> Self {
        GfxError::Sys(errno.raw_os_error())
    }
}

impl From<std::io::Error> for GfxError {
    fn from(error: std::io::Error) -> Self {
        GfxError::Sys(error.raw_os_error().unwrap_or(5))
    }
}

impl From<GfxError> for HalError {
    fn from(error: GfxError) -> Self {
        match error {
            GfxError::Sys(code) => HalError::Io(code),
            GfxError::NoDevice => HalError::NotFound,
            GfxError::UnsupportedFormat => HalError::NotSupported,
            _ => HalError::InvalidArgument,
        }
    }
}

impl fmt::Display for GfxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GfxError::Sys(code) => write!(f, "{} ({code})", self.key()),
            GfxError::TooSmall { needed, actual } => {
                write!(f, "{} ({actual} < {needed})", self.key())
            }
            other => f.write_str(other.key()),
        }
    }
}

impl std::error::Error for GfxError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_distinct_and_errors_map_to_hal() {
        let all = [
            GfxError::Sys(1),
            GfxError::InvalidArgument,
            GfxError::NotSealed,
            GfxError::TooSmall {
                needed: 2,
                actual: 1,
            },
            GfxError::TooLarge,
            GfxError::NoDevice,
            GfxError::UnsupportedFormat,
        ];
        let mut keys: Vec<_> = all.iter().map(|e| e.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), all.len());
        assert_eq!(HalError::from(GfxError::Sys(9)), HalError::Io(9));
        assert_eq!(HalError::from(GfxError::NoDevice), HalError::NotFound);
        assert!(GfxError::TooSmall {
            needed: 8,
            actual: 4
        }
        .to_string()
        .contains("4 < 8"));
        let io = std::io::Error::from_raw_os_error(13);
        assert_eq!(GfxError::from(io), GfxError::Sys(13));
    }
}
