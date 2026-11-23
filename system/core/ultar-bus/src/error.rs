//! Error types of the bus: the local [`BusError`] and the wire-level [`ErrorCode`].

use serde::{Deserialize, Serialize};
use std::fmt;

/// Typed error code carried inside `Error` envelopes.
///
/// New variants are only ever appended, so older peers keep decoding the
/// codes they know.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum ErrorCode {
    /// The policy does not grant the identity this operation (default deny).
    PermissionDenied,
    /// No connected client has registered the requested service.
    UnknownService,
    /// The service does not know the requested method.
    UnknownMethod,
    /// The service's inbound queue is full; retry later.
    ServiceBusy,
    /// The service disconnected before it answered.
    ServiceGone,
    /// The receiver did not drain its queue in time and was dropped.
    SlowConsumer,
    /// A frame exceeded the negotiated maximum size.
    FrameTooLarge,
    /// A frame could not be decoded.
    Malformed,
    /// A well-formed frame that is not allowed at this point of the protocol.
    ProtocolViolation,
    /// Another connection already owns the service name.
    NameTaken,
    /// A topic, service, method or identity name is syntactically invalid.
    InvalidName,
    /// The peer could not be mapped to an identity.
    AuthFailed,
    /// A broker limit (connections, subscriptions, retained topics) was hit.
    LimitExceeded,
    /// The broker is shutting down.
    ShuttingDown,
    /// The message contract version differs between the two sides.
    VersionMismatch,
    /// The service dropped the request without answering.
    NoReply,
    /// The payload did not decode as the expected message type.
    BadPayload,
    /// Service-defined failure code (meaning is part of the service contract).
    Service(u32),
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// Every failure a bus operation can report.
#[derive(Debug)]
#[non_exhaustive]
pub enum BusError {
    /// Underlying socket or file error.
    Io(std::io::Error),
    /// The connection was closed (by the peer or by `close`).
    Closed,
    /// The client is currently not connected (it may be reconnecting).
    NotConnected,
    /// No answer arrived within the given time.
    Timeout,
    /// A frame is larger than the allowed maximum.
    FrameTooLarge {
        /// Length announced by the frame header.
        len: u32,
        /// Largest accepted length.
        max: u32,
    },
    /// A frame or payload could not be decoded.
    Malformed,
    /// The peer did not start with the bus magic bytes.
    BadMagic,
    /// The two sides speak different wire protocol versions.
    ProtocolMismatch {
        /// Version implemented by this side.
        ours: u16,
        /// Version announced by the peer.
        theirs: u16,
    },
    /// A message arrived with a different contract version than expected.
    MessageVersion {
        /// Version compiled into this side.
        expected: u16,
        /// Version found on the wire.
        found: u16,
    },
    /// The broker or the service answered with a typed error.
    Remote(ErrorCode),
    /// A topic, service, method or identity name is syntactically invalid.
    InvalidName,
    /// A value could not be serialised.
    Encode,
    /// The subscription lost this many events because its buffer was full
    /// or because the broker dropped the connection as too slow.
    Lagged(u64),
    /// The peer could not be mapped to an identity.
    AuthFailed,
    /// The socket path is already served by a live broker.
    AddressInUse,
    /// Something that is not a socket lies at the socket path; it is not
    /// removed.
    NotASocket,
}

impl fmt::Display for BusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "bus i/o error: {err}"),
            Self::Closed => f.write_str("bus connection closed"),
            Self::NotConnected => f.write_str("bus not connected"),
            Self::Timeout => f.write_str("bus operation timed out"),
            Self::FrameTooLarge { len, max } => write!(f, "frame of {len} bytes exceeds {max}"),
            Self::Malformed => f.write_str("malformed bus frame"),
            Self::BadMagic => f.write_str("peer is not an ultar-bus endpoint"),
            Self::ProtocolMismatch { ours, theirs } => {
                write!(
                    f,
                    "protocol version mismatch (ours {ours}, theirs {theirs})"
                )
            }
            Self::MessageVersion { expected, found } => {
                write!(
                    f,
                    "message version mismatch (expected {expected}, found {found})"
                )
            }
            Self::Remote(code) => write!(f, "bus error reply: {code}"),
            Self::InvalidName => f.write_str("invalid bus name"),
            Self::Encode => f.write_str("message could not be encoded"),
            Self::Lagged(lost) => write!(f, "subscription lagged, {lost} event(s) lost"),
            Self::AuthFailed => f.write_str("peer could not be authenticated"),
            Self::AddressInUse => f.write_str("bus socket already in use"),
            Self::NotASocket => {
                f.write_str("socket path is occupied by something that is not a socket")
            }
        }
    }
}

impl std::error::Error for BusError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for BusError {
    fn from(err: std::io::Error) -> Self {
        use std::io::ErrorKind;
        match err.kind() {
            ErrorKind::UnexpectedEof
            | ErrorKind::BrokenPipe
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted => Self::Closed,
            ErrorKind::WouldBlock | ErrorKind::TimedOut => Self::Timeout,
            _ => Self::Io(err),
        }
    }
}

impl BusError {
    /// The wire code that best describes this error to a remote peer.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::FrameTooLarge { .. } => ErrorCode::FrameTooLarge,
            Self::Remote(code) => *code,
            Self::InvalidName => ErrorCode::InvalidName,
            Self::MessageVersion { .. } | Self::ProtocolMismatch { .. } => {
                ErrorCode::VersionMismatch
            }
            Self::AuthFailed => ErrorCode::AuthFailed,
            Self::Encode => ErrorCode::BadPayload,
            _ => ErrorCode::Malformed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};

    #[test]
    fn io_errors_map_to_typed_variants() {
        assert!(matches!(
            BusError::from(Error::from(ErrorKind::UnexpectedEof)),
            BusError::Closed
        ));
        assert!(matches!(
            BusError::from(Error::from(ErrorKind::BrokenPipe)),
            BusError::Closed
        ));
        assert!(matches!(
            BusError::from(Error::from(ErrorKind::WouldBlock)),
            BusError::Timeout
        ));
        assert!(matches!(
            BusError::from(Error::from(ErrorKind::NotFound)),
            BusError::Io(_)
        ));
    }

    #[test]
    fn codes_describe_errors() {
        assert_eq!(
            BusError::FrameTooLarge { len: 9, max: 1 }.code(),
            ErrorCode::FrameTooLarge
        );
        assert_eq!(
            BusError::Remote(ErrorCode::NameTaken).code(),
            ErrorCode::NameTaken
        );
        assert_eq!(BusError::InvalidName.code(), ErrorCode::InvalidName);
        assert_eq!(BusError::Malformed.code(), ErrorCode::Malformed);
        assert_eq!(
            BusError::MessageVersion {
                expected: 1,
                found: 2
            }
            .code(),
            ErrorCode::VersionMismatch
        );
    }

    #[test]
    fn display_and_source_work() {
        let err = BusError::Io(Error::from(ErrorKind::NotFound));
        assert!(std::error::Error::source(&err).is_some());
        assert!(!err.to_string().is_empty());
        assert!(std::error::Error::source(&BusError::Closed).is_none());
        assert_eq!(ErrorCode::Service(7).to_string(), "Service(7)");
        for err in [
            BusError::Lagged(3),
            BusError::Timeout,
            BusError::AddressInUse,
        ] {
            assert!(!err.to_string().is_empty());
        }
    }

    #[test]
    fn error_codes_round_trip_on_the_wire() {
        for code in [
            ErrorCode::PermissionDenied,
            ErrorCode::SlowConsumer,
            ErrorCode::Service(42),
        ] {
            let bytes = postcard::to_allocvec(&code).unwrap();
            assert_eq!(postcard::from_bytes::<ErrorCode>(&bytes).unwrap(), code);
        }
    }
}
