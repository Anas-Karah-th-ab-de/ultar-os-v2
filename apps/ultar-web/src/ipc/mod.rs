//! The protocol between the browser and the engine host (`external/wpe-webkit/host`):
//! frames of `u32 length (LE) · u8 kind · payload` over a Unix stream socket. Commands go to
//! the host, events come back. The only file descriptor ever passed is the shared-memory
//! frame buffer of a tab (with [`HostEvent::Buffer`], `SCM_RIGHTS`). Everything the host
//! sends is checked here: sizes, kinds, values, UTF-8; a violation ends the connection.
//! The C side mirrors this module in `external/wpe-webkit/host/proto.h`.

mod codec;
mod messages;

pub use codec::{Reader, Writer, MAX_STRING};
pub use messages::{Command, HostEvent, PROTOCOL_VERSION};

use std::fmt;

/// Largest message (length field) in bytes.
pub const MAX_MESSAGE: usize = 64 * 1024;
/// Largest frame buffer side in pixels.
pub const MAX_SIDE: u32 = 16_384;

/// Why bytes from the host were refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The message ends early.
    Truncated,
    /// Length field 0 or above [`MAX_MESSAGE`].
    BadLength,
    /// Unknown message kind.
    UnknownKind(u8),
    /// A value out of range (flag, size, kind code, non-finite float).
    BadValue,
    /// A string longer than [`MAX_STRING`].
    TooLong,
    /// A string that is not UTF-8.
    BadUtf8,
    /// Bytes after the last field.
    TrailingBytes,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Truncated => f.write_str("message truncated"),
            DecodeError::BadLength => f.write_str("bad message length"),
            DecodeError::UnknownKind(k) => write!(f, "unknown message kind {k}"),
            DecodeError::BadValue => f.write_str("value out of range"),
            DecodeError::TooLong => f.write_str("string too long"),
            DecodeError::BadUtf8 => f.write_str("string not UTF-8"),
            DecodeError::TrailingBytes => f.write_str("trailing bytes"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Collects bytes from the socket and cuts them into messages.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    /// Adds received bytes.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// The next whole message as `(kind, payload)`, `Ok(None)` when more bytes are needed.
    pub fn next_message(&mut self) -> Result<Option<(u8, Vec<u8>)>, DecodeError> {
        if self.buf.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
        if len == 0 || len > MAX_MESSAGE {
            return Err(DecodeError::BadLength);
        }
        if self.buf.len() < 4 + len {
            return Ok(None);
        }
        let kind = self.buf[4];
        let payload = self.buf[5..4 + len].to_vec();
        self.buf.drain(..4 + len);
        Ok(Some((kind, payload)))
    }

    /// Bytes waiting for the rest of a message.
    pub fn pending(&self) -> usize {
        self.buf.len()
    }
}

#[cfg(test)]
mod tests;
