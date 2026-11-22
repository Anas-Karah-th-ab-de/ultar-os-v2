//! Wire format: preamble, length-prefixed frames and the typed [`Envelope`].
//!
//! ```text
//! connection start (both directions, once):  "UBUS" | version u16 LE | 0u16
//! afterwards, repeated:                      length u32 LE | postcard(Envelope)
//! ```

use crate::error::{BusError, ErrorCode};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// Version of the wire protocol (preamble, framing, envelope layout).
pub const PROTOCOL_VERSION: u16 = 1;
/// Magic bytes that open every connection.
pub const MAGIC: [u8; 4] = *b"UBUS";
/// Default upper bound for one frame body in bytes.
pub const DEFAULT_MAX_FRAME: u32 = 1 << 20;
/// Envelope flag: the broker keeps the event as last value of its topic.
pub const FLAG_RETAIN: u8 = 0b0000_0001;
/// Envelope flag: the sender wants a `Response` acknowledging the event.
pub const FLAG_ACK: u8 = 0b0000_0010;

const HEADER_LEN: usize = 4;

/// What an [`Envelope`] means.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Client → broker: credentials after the preamble.
    Hello,
    /// Broker → client: handshake accepted.
    Welcome,
    /// Client → broker: claim a service name.
    Register,
    /// Client → broker: release a service name.
    Unregister,
    /// Call of a service method.
    Request,
    /// Successful answer to a request, or acknowledgement.
    Response,
    /// Publication on a topic.
    Event,
    /// Client → broker: start receiving a topic or prefix.
    Subscribe,
    /// Client → broker: stop a subscription.
    Unsubscribe,
    /// Typed failure; the payload is an [`ErrorCode`].
    Error,
}

/// The one message shape that travels on the bus.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Envelope {
    /// Meaning of this envelope.
    pub kind: Kind,
    /// Correlation id (request id, subscription id; `0` when unused).
    pub id: u64,
    /// Service name, topic name or subscription pattern.
    pub target: String,
    /// Method name for requests, empty otherwise.
    pub method: String,
    /// Contract version of the payload ([`crate::Message::VERSION`]).
    pub version: u16,
    /// Bit set of `FLAG_*` values.
    pub flags: u8,
    /// Identity of the caller; filled in by the broker on forwarded requests.
    pub origin: String,
    /// Serialised message body.
    #[serde(with = "payload_bytes")]
    pub payload: Vec<u8>,
}

impl Envelope {
    /// An envelope of `kind` with correlation `id` and all other fields empty.
    pub fn new(kind: Kind, id: u64) -> Self {
        Self {
            kind,
            id,
            target: String::new(),
            method: String::new(),
            version: 0,
            flags: 0,
            origin: String::new(),
            payload: Vec::new(),
        }
    }

    /// Sets the target (service, topic or pattern).
    pub fn with_target(mut self, target: &str) -> Self {
        self.target = target.to_owned();
        self
    }

    /// Sets the payload and its contract version.
    pub fn with_payload(mut self, version: u16, payload: Vec<u8>) -> Self {
        self.version = version;
        self.payload = payload;
        self
    }

    /// An `Error` envelope carrying `code` for correlation `id`.
    pub fn error(id: u64, code: ErrorCode) -> Self {
        let mut envelope = Self::new(Kind::Error, id);
        // Encoding a field-less or u32 enum cannot fail; an empty payload
        // decodes as `Malformed` on the other side.
        envelope.payload = postcard::to_allocvec(&code).unwrap_or_default();
        envelope
    }

    /// The error code of an `Error` envelope.
    pub fn error_code(&self) -> ErrorCode {
        postcard::from_bytes(&self.payload).unwrap_or(ErrorCode::Malformed)
    }

    /// Serialises the envelope into one complete frame (header + body).
    pub fn to_frame(&self, max_frame: u32) -> Result<Vec<u8>, BusError> {
        let mut frame = Vec::with_capacity(HEADER_LEN + 24 + self.payload.len());
        frame.extend_from_slice(&[0u8; HEADER_LEN]);
        let mut frame = postcard::to_extend(self, frame).map_err(|_| BusError::Encode)?;
        let body = frame.len() - HEADER_LEN;
        let len = u32::try_from(body).unwrap_or(u32::MAX);
        if len > max_frame {
            return Err(BusError::FrameTooLarge {
                len,
                max: max_frame,
            });
        }
        frame[..HEADER_LEN].copy_from_slice(&len.to_le_bytes());
        Ok(frame)
    }

    /// Decodes a frame body (without header).
    pub fn from_body(body: &[u8]) -> Result<Self, BusError> {
        postcard::from_bytes(body).map_err(|_| BusError::Malformed)
    }
}

/// Payload of the `Hello` envelope.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Hello {
    /// Free-form diagnostic name of the client program.
    pub client_name: String,
    /// Optional secret for token based authentication.
    pub token: Option<String>,
}

/// Payload of the `Welcome` envelope.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Welcome {
    /// Identity the broker mapped this connection to.
    pub identity: String,
    /// Largest frame body the broker accepts.
    pub max_frame: u32,
}

/// Writes the connection preamble announcing `version`.
pub fn write_preamble(writer: &mut impl Write, version: u16) -> Result<(), BusError> {
    let mut bytes = [0u8; 8];
    bytes[..4].copy_from_slice(&MAGIC);
    bytes[4..6].copy_from_slice(&version.to_le_bytes());
    writer.write_all(&bytes)?;
    Ok(())
}

/// Reads the peer's preamble and returns the protocol version it announced.
pub fn read_preamble(reader: &mut impl Read) -> Result<u16, BusError> {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes)?;
    if bytes[..4] != MAGIC {
        return Err(BusError::BadMagic);
    }
    Ok(u16::from_le_bytes([bytes[4], bytes[5]]))
}

/// Reads one frame body, enforcing `max_frame`.
pub fn read_frame(reader: &mut impl Read, max_frame: u32) -> Result<Vec<u8>, BusError> {
    let mut header = [0u8; HEADER_LEN];
    reader.read_exact(&mut header)?;
    let len = u32::from_le_bytes(header);
    if len == 0 {
        return Err(BusError::Malformed);
    }
    if len > max_frame {
        return Err(BusError::FrameTooLarge {
            len,
            max: max_frame,
        });
    }
    let mut body = vec![0u8; len as usize];
    reader.read_exact(&mut body)?;
    Ok(body)
}

/// Reads and decodes one envelope.
pub fn read_envelope(reader: &mut impl Read, max_frame: u32) -> Result<Envelope, BusError> {
    Envelope::from_body(&read_frame(reader, max_frame)?)
}

/// Encodes and writes one envelope with a single `write_all`.
pub fn write_envelope(
    writer: &mut impl Write,
    envelope: &Envelope,
    max_frame: u32,
) -> Result<(), BusError> {
    writer.write_all(&envelope.to_frame(max_frame)?)?;
    Ok(())
}

/// Serde adapter that stores `Vec<u8>` as one byte string instead of a
/// sequence of single elements.
mod payload_bytes {
    use serde::de::{Error, SeqAccess, Visitor};
    use serde::{Deserializer, Serializer};
    use std::fmt;

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(bytes)
    }

    struct BytesVisitor;

    impl<'de> Visitor<'de> for BytesVisitor {
        type Value = Vec<u8>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a byte string")
        }

        fn visit_bytes<E: Error>(self, bytes: &[u8]) -> Result<Vec<u8>, E> {
            Ok(bytes.to_vec())
        }

        fn visit_byte_buf<E: Error>(self, bytes: Vec<u8>) -> Result<Vec<u8>, E> {
            Ok(bytes)
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
            let mut bytes = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(4096));
            while let Some(byte) = seq.next_element()? {
                bytes.push(byte);
            }
            Ok(bytes)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        deserializer.deserialize_byte_buf(BytesVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn sample() -> Envelope {
        let mut envelope = Envelope::new(Kind::Request, 77)
            .with_target("settings")
            .with_payload(3, vec![1, 2, 3, 255]);
        envelope.method = "get".into();
        envelope.flags = FLAG_ACK;
        envelope.origin = "app.demo".into();
        envelope
    }

    #[test]
    fn envelope_round_trips_through_a_frame() {
        let frame = sample().to_frame(DEFAULT_MAX_FRAME).unwrap();
        let mut cursor = Cursor::new(frame);
        assert_eq!(
            read_envelope(&mut cursor, DEFAULT_MAX_FRAME).unwrap(),
            sample()
        );
    }

    #[test]
    fn write_then_read_several_envelopes() {
        let mut buffer = Vec::new();
        for id in 1..=3 {
            write_envelope(&mut buffer, &Envelope::new(Kind::Event, id), 1024).unwrap();
        }
        let mut cursor = Cursor::new(buffer);
        for id in 1..=3 {
            assert_eq!(read_envelope(&mut cursor, 1024).unwrap().id, id);
        }
        assert!(matches!(
            read_envelope(&mut cursor, 1024),
            Err(BusError::Closed)
        ));
    }

    #[test]
    fn oversized_frames_are_rejected_on_both_sides() {
        let big = Envelope::new(Kind::Event, 1).with_payload(1, vec![0; 200]);
        assert!(matches!(
            big.to_frame(64),
            Err(BusError::FrameTooLarge { max: 64, .. })
        ));
        let frame = big.to_frame(1024).unwrap();
        let mut cursor = Cursor::new(frame);
        assert!(matches!(
            read_frame(&mut cursor, 64),
            Err(BusError::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn empty_and_truncated_frames_fail_cleanly() {
        let mut empty = Cursor::new(vec![0u8, 0, 0, 0]);
        assert!(matches!(
            read_frame(&mut empty, 64),
            Err(BusError::Malformed)
        ));
        let mut truncated = Cursor::new(vec![10u8, 0, 0, 0, 1, 2]);
        assert!(matches!(
            read_frame(&mut truncated, 64),
            Err(BusError::Closed)
        ));
        assert!(matches!(
            Envelope::from_body(&[0xff, 0xff, 0xff]),
            Err(BusError::Malformed)
        ));
    }

    #[test]
    fn preamble_round_trip_and_bad_magic() {
        let mut buffer = Vec::new();
        write_preamble(&mut buffer, 9).unwrap();
        assert_eq!(read_preamble(&mut Cursor::new(buffer)).unwrap(), 9);
        let mut junk = Cursor::new(b"HTTP/1.1".to_vec());
        assert!(matches!(read_preamble(&mut junk), Err(BusError::BadMagic)));
        let mut short = Cursor::new(b"UB".to_vec());
        assert!(matches!(read_preamble(&mut short), Err(BusError::Closed)));
    }

    #[test]
    fn error_envelopes_carry_their_code() {
        let envelope = Envelope::error(5, ErrorCode::PermissionDenied);
        assert_eq!(envelope.kind, Kind::Error);
        assert_eq!(envelope.id, 5);
        assert_eq!(envelope.error_code(), ErrorCode::PermissionDenied);
        assert_eq!(
            Envelope::new(Kind::Error, 1).error_code(),
            ErrorCode::Malformed
        );
    }

    #[test]
    fn payload_is_stored_as_one_byte_string() {
        let small = Envelope::new(Kind::Event, 1).with_payload(1, vec![7; 300]);
        let frame = small.to_frame(DEFAULT_MAX_FRAME).unwrap();
        // 300 payload bytes + small fixed overhead, not one varint per byte.
        assert!(frame.len() < 300 + 20, "frame is {} bytes", frame.len());
    }

    #[test]
    fn hello_and_welcome_round_trip() {
        let hello = Hello {
            client_name: "t".into(),
            token: Some("s".into()),
        };
        let bytes = postcard::to_allocvec(&hello).unwrap();
        assert_eq!(postcard::from_bytes::<Hello>(&bytes).unwrap(), hello);
        let welcome = Welcome {
            identity: "system.devd".into(),
            max_frame: 9,
        };
        let bytes = postcard::to_allocvec(&welcome).unwrap();
        assert_eq!(postcard::from_bytes::<Welcome>(&bytes).unwrap(), welcome);
    }
}
