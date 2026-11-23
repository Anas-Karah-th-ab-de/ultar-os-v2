//! Errors of the client library.

use crate::proto::ErrorCode;
use crate::wire::WireError;
use std::fmt;
use ultar_hal_gfx::GfxError;

/// What went wrong on the client side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientError {
    /// The socket failed; carries the error number.
    Io(i32),
    /// The compositor closed the connection (it may be restarting).
    Disconnected,
    /// No answer in time.
    Timeout,
    /// A message from the compositor was malformed.
    Wire(WireError),
    /// Shared memory could not be created.
    Gfx(GfxError),
    /// The compositor refused the client and closed the connection.
    Refused(ErrorCode),
    /// The compositor speaks another protocol version.
    Version(u16),
    /// A size is outside what the protocol allows.
    InvalidSize,
}

impl ClientError {
    /// Stable key of the error.
    pub const fn key(self) -> &'static str {
        match self {
            ClientError::Io(_) => "surface.client.io",
            ClientError::Disconnected => "surface.client.disconnected",
            ClientError::Timeout => "surface.client.timeout",
            ClientError::Wire(_) => "surface.client.wire",
            ClientError::Gfx(_) => "surface.client.gfx",
            ClientError::Refused(_) => "surface.client.refused",
            ClientError::Version(_) => "surface.client.version",
            ClientError::InvalidSize => "surface.client.invalid_size",
        }
    }
}

impl From<std::io::Error> for ClientError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::UnexpectedEof => ClientError::Disconnected,
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => ClientError::Timeout,
            _ => ClientError::Io(e.raw_os_error().unwrap_or(5)),
        }
    }
}

impl From<WireError> for ClientError {
    fn from(e: WireError) -> Self {
        ClientError::Wire(e)
    }
}

impl From<GfxError> for ClientError {
    fn from(e: GfxError) -> Self {
        ClientError::Gfx(e)
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Io(n) => write!(f, "{} ({n})", self.key()),
            ClientError::Wire(w) => write!(f, "{}: {w}", self.key()),
            ClientError::Gfx(g) => write!(f, "{}: {g}", self.key()),
            ClientError::Refused(c) => write!(f, "{}: {c:?}", self.key()),
            ClientError::Version(v) => write!(f, "{} ({v})", self.key()),
            other => f.write_str(other.key()),
        }
    }
}

impl std::error::Error for ClientError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_errors_map_to_meaning() {
        let e = |k| ClientError::from(std::io::Error::from(k));
        assert_eq!(e(std::io::ErrorKind::BrokenPipe), ClientError::Disconnected);
        assert_eq!(e(std::io::ErrorKind::TimedOut), ClientError::Timeout);
        assert_eq!(
            ClientError::from(std::io::Error::from_raw_os_error(2)),
            ClientError::Io(2)
        );
        assert!(ClientError::Refused(ErrorCode::Limit)
            .to_string()
            .contains("Limit"));
        assert_eq!(
            ClientError::from(WireError::Trailing),
            ClientError::Wire(WireError::Trailing)
        );
        assert_eq!(
            ClientError::from(GfxError::NotSealed),
            ClientError::Gfx(GfxError::NotSealed)
        );
        assert_ne!(ClientError::Timeout.key(), ClientError::Disconnected.key());
    }
}
