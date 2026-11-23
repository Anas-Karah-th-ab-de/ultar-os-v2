//! Wire format between a service and the log service (version 1).
//!
//! ```text
//! frame  := len:u16le body            len = 1 ..= MAX_BODY
//! body   := version:u8 (=1) level:u8 key:text nfields:u8 field*
//! field  := name:text tag:u8 value
//! value  := tag 0: u64le | tag 1: i64le | tag 2: u8 (0|1)
//!         | tag 3: text (word) | tag 4: text (short identifier)
//! text   := len:u8 bytes
//! ```
//!
//! The decoder is strict: unknown version, level or tag, a text outside
//! its character set, more than [`MAX_FIELDS`] fields, a truncated body or
//! bytes after the last field all refuse the record. It is fuzzed with a
//! fixed-seed generator (`tests/log_wire_fuzz.rs`).

use super::record::{is_key, Level, Record, ShortId, Value, MAX_FIELDS};
use std::fmt;

/// Version byte of this format.
pub const WIRE_VERSION: u8 = 1;
/// Largest body of one record in bytes.
pub const MAX_BODY: usize = 1024;

/// Why a record was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WireError {
    /// The body is empty or longer than [`MAX_BODY`].
    Size,
    /// Unknown format version.
    Version,
    /// Unknown level number.
    Level,
    /// A key, field name or word is not a key (see [`is_key`]).
    Key,
    /// A short identifier breaks its rules.
    Id,
    /// More than [`MAX_FIELDS`] fields.
    Fields,
    /// Unknown value tag or a flag that is neither 0 nor 1.
    Value,
    /// The body ends in the middle of a field.
    Truncated,
    /// Bytes after the last field.
    Trailing,
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Size => "log record size out of bounds",
            Self::Version => "unknown log record version",
            Self::Level => "unknown log level",
            Self::Key => "invalid key in log record",
            Self::Id => "invalid identifier in log record",
            Self::Fields => "too many fields in log record",
            Self::Value => "invalid value in log record",
            Self::Truncated => "truncated log record",
            Self::Trailing => "trailing bytes in log record",
        })
    }
}

impl std::error::Error for WireError {}

/// A value as the log service holds it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum OwnedValue {
    /// Unsigned quantity.
    Count(u64),
    /// Signed quantity.
    Signed(i64),
    /// Yes or no.
    Flag(bool),
    /// A word (validated key syntax).
    Word(String),
    /// A short identifier (validated).
    Id(String),
}

/// A decoded record: every text in it has passed validation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OwnedRecord {
    /// Severity.
    pub level: Level,
    /// Message key.
    pub key: String,
    /// Fields in wire order.
    pub fields: Vec<(String, OwnedValue)>,
}

fn put_text(out: &mut Vec<u8>, text: &str) {
    out.push(text.len() as u8);
    out.extend_from_slice(text.as_bytes());
}

fn put_key(out: &mut Vec<u8>, text: &str) -> Result<(), WireError> {
    if !is_key(text) {
        return Err(WireError::Key);
    }
    put_text(out, text);
    Ok(())
}

/// Encodes the body of `record` (without the length prefix).
pub fn encode_body(record: &Record) -> Result<Vec<u8>, WireError> {
    if record.overflowed() {
        return Err(WireError::Fields);
    }
    let mut out = Vec::with_capacity(96);
    out.push(WIRE_VERSION);
    out.push(record.level().number());
    put_key(&mut out, record.key())?;
    out.push(record.fields().len() as u8);
    for (name, value) in record.fields() {
        put_key(&mut out, name)?;
        match value {
            Value::Count(n) => {
                out.push(0);
                out.extend_from_slice(&n.to_le_bytes());
            }
            Value::Signed(n) => {
                out.push(1);
                out.extend_from_slice(&n.to_le_bytes());
            }
            Value::Flag(flag) => out.extend_from_slice(&[2, u8::from(*flag)]),
            Value::Word(word) => {
                out.push(3);
                put_key(&mut out, word)?;
            }
            Value::Id(id) => {
                out.push(4);
                put_text(&mut out, id.as_str());
            }
        }
    }
    if out.len() > MAX_BODY {
        return Err(WireError::Size);
    }
    Ok(out)
}

/// Encodes `record` as one frame (length prefix and body).
pub fn encode_frame(record: &Record) -> Result<Vec<u8>, WireError> {
    let body = encode_body(record)?;
    let mut frame = Vec::with_capacity(body.len() + 2);
    frame.extend_from_slice(&(body.len() as u16).to_le_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

struct Cursor<'a> {
    bytes: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], WireError> {
        if self.bytes.len() < count {
            return Err(WireError::Truncated);
        }
        let (head, tail) = self.bytes.split_at(count);
        self.bytes = tail;
        Ok(head)
    }

    fn byte(&mut self) -> Result<u8, WireError> {
        Ok(self.take(1)?[0])
    }

    fn eight(&mut self) -> Result<[u8; 8], WireError> {
        let mut out = [0u8; 8];
        out.copy_from_slice(self.take(8)?);
        Ok(out)
    }

    fn text(&mut self) -> Result<&'a str, WireError> {
        let len = usize::from(self.byte()?);
        std::str::from_utf8(self.take(len)?).map_err(|_| WireError::Key)
    }

    fn key(&mut self) -> Result<String, WireError> {
        let text = self.text()?;
        if !is_key(text) {
            return Err(WireError::Key);
        }
        Ok(text.to_owned())
    }

    fn value(&mut self) -> Result<OwnedValue, WireError> {
        Ok(match self.byte()? {
            0 => OwnedValue::Count(u64::from_le_bytes(self.eight()?)),
            1 => OwnedValue::Signed(i64::from_le_bytes(self.eight()?)),
            2 => match self.byte()? {
                0 => OwnedValue::Flag(false),
                1 => OwnedValue::Flag(true),
                _ => return Err(WireError::Value),
            },
            3 => OwnedValue::Word(self.key()?),
            4 => {
                let text = self.text().map_err(|error| match error {
                    WireError::Key => WireError::Id,
                    other => other,
                })?;
                if !ShortId::is_valid(text) {
                    return Err(WireError::Id);
                }
                OwnedValue::Id(text.to_owned())
            }
            _ => return Err(WireError::Value),
        })
    }
}

/// Decodes one body. Never panics, never allocates more than the body
/// itself justifies.
pub fn decode_body(body: &[u8]) -> Result<OwnedRecord, WireError> {
    if body.is_empty() || body.len() > MAX_BODY {
        return Err(WireError::Size);
    }
    let mut cursor = Cursor { bytes: body };
    if cursor.byte()? != WIRE_VERSION {
        return Err(WireError::Version);
    }
    let level = Level::from_number(cursor.byte()?).ok_or(WireError::Level)?;
    let key = cursor.key()?;
    let count = usize::from(cursor.byte()?);
    if count > MAX_FIELDS {
        return Err(WireError::Fields);
    }
    let mut fields = Vec::with_capacity(count);
    for _ in 0..count {
        let name = cursor.key()?;
        fields.push((name, cursor.value()?));
    }
    if !cursor.bytes.is_empty() {
        return Err(WireError::Trailing);
    }
    Ok(OwnedRecord { level, key, fields })
}

/// Cuts a byte stream into frames. Holds at most one frame
/// (`MAX_BODY + 2` bytes) however the bytes arrive.
#[derive(Default)]
pub struct FrameReader {
    buffer: Vec<u8>,
}

impl FrameReader {
    /// An empty reader.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds `input`; calls `sink` with every complete body. A frame
    /// length of 0 or above [`MAX_BODY`] is [`WireError::Size`] – the
    /// stream cannot be resynchronised after that and must be closed.
    pub fn push(&mut self, mut input: &[u8], mut sink: impl FnMut(&[u8])) -> Result<(), WireError> {
        while !input.is_empty() {
            let needed = match self.buffer.len() {
                0 | 1 => 2 - self.buffer.len(),
                have => {
                    let len = usize::from(u16::from_le_bytes([self.buffer[0], self.buffer[1]]));
                    len + 2 - have
                }
            };
            let take = needed.min(input.len());
            self.buffer.extend_from_slice(&input[..take]);
            input = &input[take..];
            if self.buffer.len() < 2 {
                continue;
            }
            let len = usize::from(u16::from_le_bytes([self.buffer[0], self.buffer[1]]));
            if len == 0 || len > MAX_BODY {
                self.buffer.clear();
                return Err(WireError::Size);
            }
            if self.buffer.len() == len + 2 {
                sink(&self.buffer[2..]);
                self.buffer.clear();
            }
        }
        Ok(())
    }

    /// Bytes of an incomplete frame currently held.
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Record {
        Record::new(Level::Warn, "bus.refused")
            .word("op", "publish")
            .id("peer", ShortId::new("app.phone").unwrap())
            .count("n", u64::MAX)
            .signed("code", i64::MIN)
            .flag("retained", true)
    }

    #[test]
    fn records_round_trip() {
        let body = encode_body(&sample()).unwrap();
        let decoded = decode_body(&body).unwrap();
        assert_eq!(decoded.level, Level::Warn);
        assert_eq!(decoded.key, "bus.refused");
        assert_eq!(
            decoded.fields,
            vec![
                ("op".to_owned(), OwnedValue::Word("publish".to_owned())),
                ("peer".to_owned(), OwnedValue::Id("app.phone".to_owned())),
                ("n".to_owned(), OwnedValue::Count(u64::MAX)),
                ("code".to_owned(), OwnedValue::Signed(i64::MIN)),
                ("retained".to_owned(), OwnedValue::Flag(true)),
            ]
        );
        let empty = decode_body(&encode_body(&Record::new(Level::Debug, "a")).unwrap()).unwrap();
        assert!(empty.fields.is_empty());
    }

    #[test]
    fn the_encoder_refuses_bad_keys_and_overflow() {
        for record in [
            Record::new(Level::Info, "Bad Key"),
            Record::new(Level::Info, ""),
            Record::new(Level::Info, "ok").count("Bad Name", 1),
            Record::new(Level::Info, "ok").word("w", "has space"),
        ] {
            assert_eq!(encode_body(&record), Err(WireError::Key));
        }
        let mut over = Record::new(Level::Info, "ok");
        for _ in 0..=MAX_FIELDS {
            over = over.count("n", 1);
        }
        assert_eq!(encode_frame(&over), Err(WireError::Fields));
    }

    #[test]
    fn the_largest_record_fits_the_body_limit() {
        const KEY: &str = "kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk";
        assert_eq!(KEY.len(), super::super::record::MAX_KEY_LEN);
        let mut record = Record::new(Level::Critical, KEY);
        for _ in 0..MAX_FIELDS {
            record = record.word(KEY, KEY);
        }
        let body = encode_body(&record).unwrap();
        assert!(body.len() <= MAX_BODY, "{}", body.len());
        assert!(decode_body(&body).is_ok());
    }

    #[test]
    fn every_truncation_and_every_extra_byte_is_refused() {
        let body = encode_body(&sample()).unwrap();
        for cut in 0..body.len() {
            assert!(decode_body(&body[..cut]).is_err(), "cut at {cut}");
        }
        let mut longer = body.clone();
        longer.push(0);
        assert_eq!(decode_body(&longer), Err(WireError::Trailing));
        assert_eq!(decode_body(&vec![1; MAX_BODY + 1]), Err(WireError::Size));
    }

    #[test]
    fn hostile_bodies_get_typed_errors() {
        let ok = encode_body(&Record::new(Level::Info, "ab").flag("f", true)).unwrap();
        // version, level, key len, 'a', 'b', nfields, name len, 'f', tag, value
        let patched = |index: usize, value: u8| {
            let mut body = ok.clone();
            body[index] = value;
            decode_body(&body)
        };
        assert_eq!(patched(0, 2), Err(WireError::Version));
        assert_eq!(patched(1, 6), Err(WireError::Level));
        assert_eq!(patched(3, b'A'), Err(WireError::Key));
        assert_eq!(patched(3, 0xff), Err(WireError::Key));
        assert_eq!(patched(5, 9), Err(WireError::Fields));
        assert_eq!(patched(7, b' '), Err(WireError::Key));
        assert_eq!(patched(8, 5), Err(WireError::Value));
        assert_eq!(patched(9, 2), Err(WireError::Value));
        // A phone number as "identifier" is refused by the receiver too.
        let mut forged = vec![WIRE_VERSION, 1, 1, b'k', 1, 1, b'p', 4, 13];
        forged.extend_from_slice(b"+963115550100");
        assert_eq!(decode_body(&forged), Err(WireError::Id));
        let mut free_text = vec![WIRE_VERSION, 1, 1, b'k', 1, 1, b'p', 3, 11];
        free_text.extend_from_slice(b"Hello World");
        assert_eq!(decode_body(&free_text), Err(WireError::Key));
        for error in [
            WireError::Size,
            WireError::Version,
            WireError::Level,
            WireError::Key,
            WireError::Id,
            WireError::Fields,
            WireError::Value,
            WireError::Truncated,
            WireError::Trailing,
        ] {
            assert!(!error.to_string().is_empty());
        }
    }

    #[test]
    fn frames_are_cut_however_the_bytes_arrive() {
        let mut stream = Vec::new();
        let records = [sample(), Record::new(Level::Info, "x"), sample()];
        for record in &records {
            stream.extend_from_slice(&encode_frame(record).unwrap());
        }
        for chunk in [1, 2, 3, 7, 64, stream.len()] {
            let mut reader = FrameReader::new();
            let mut seen = Vec::new();
            for piece in stream.chunks(chunk) {
                reader
                    .push(piece, |body| seen.push(decode_body(body).unwrap().key))
                    .unwrap();
                assert!(reader.buffered() <= MAX_BODY + 2);
            }
            assert_eq!(seen, ["bus.refused", "x", "bus.refused"], "chunk {chunk}");
            assert_eq!(reader.buffered(), 0);
        }
    }

    #[test]
    fn oversized_and_empty_frames_end_the_stream() {
        let mut reader = FrameReader::new();
        assert_eq!(reader.push(&[0, 0], |_| {}), Err(WireError::Size));
        let too_long = ((MAX_BODY + 1) as u16).to_le_bytes();
        assert_eq!(reader.push(&too_long, |_| {}), Err(WireError::Size));
        assert_eq!(reader.push(&[0xff], |_| {}), Ok(()));
        assert_eq!(reader.push(&[0xff], |_| {}), Err(WireError::Size));
    }
}
