//! A record as the log service stores it, and its byte form.
//!
//! ```text
//! payload := seq:u64le time_ms:u64le kind:u8 level:u8 service:text8 key:text8
//!            nfields:u8 field* text_len:u16le text_bytes
//! field   := name:text8 tag:u8 value          (as in the client wire format)
//! ```
//!
//! The decoder validates everything again: segment files are input from
//! outside (the disk may be damaged or edited).

use ultar_service::log::record::{is_key, ShortId, MAX_FIELDS};
use ultar_service::log::{Level, OwnedRecord, OwnedValue};

/// Longest source name in bytes.
pub const MAX_SERVICE_LEN: usize = 64;
/// Longest kernel line kept, in bytes.
pub const MAX_TEXT_LEN: usize = 256;
/// Largest encoded payload.
pub const MAX_PAYLOAD: usize = 2048;

/// Where a record came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum SourceKind {
    /// A service or app, named by its verified identity.
    Service,
    /// The kernel log.
    Kernel,
    /// The log service itself.
    Internal,
}

impl SourceKind {
    /// Storage number.
    pub const fn number(self) -> u8 {
        match self {
            Self::Service => 0,
            Self::Kernel => 1,
            Self::Internal => 2,
        }
    }

    /// The kind with this storage number.
    pub const fn from_number(number: u8) -> Option<Self> {
        match number {
            0 => Some(Self::Service),
            1 => Some(Self::Kernel),
            2 => Some(Self::Internal),
            _ => None,
        }
    }

    /// Stable lower-case name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Service => "service",
            Self::Kernel => "kernel",
            Self::Internal => "internal",
        }
    }
}

/// Name of the kernel source.
pub const KERNEL_NAME: &str = "kernel";
/// Name of the log service as a source.
pub const INTERNAL_NAME: &str = "logd";

/// One stored record.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stored {
    /// Position in the log.
    pub seq: u64,
    /// Arrival time, milliseconds since 1970 UTC.
    pub time_ms: u64,
    /// Kind of source.
    pub kind: SourceKind,
    /// Name of the source.
    pub service: String,
    /// Severity.
    pub level: Level,
    /// Message key.
    pub key: String,
    /// Typed fields.
    pub fields: Vec<(String, OwnedValue)>,
    /// The kernel's line (kernel records only, otherwise empty).
    pub text: String,
}

/// The payload could not be decoded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BadPayload;

/// Whether `name` can be a source name: 1–64 bytes of `a-z 0-9 . _ -`.
pub fn is_service_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_SERVICE_LEN
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

/// Whether `character` must not reach a terminal from free text: control
/// characters, and the invisible format characters that change how a line
/// is shown – direction marks, embeddings, overrides and isolates
/// (U+061C, U+200E/F, U+202A–E, U+2066–9; review round 3, R3-L4) – and the
/// zero-width characters that hide text (U+200B–D, U+2060, U+FEFF).
pub fn is_unsafe_char(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}'
                | '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{2069}'
                | '\u{feff}'
        )
}

/// Makes a kernel line storable: at most [`MAX_TEXT_LEN`] bytes, control
/// and invisible format characters replaced by spaces (see
/// [`is_unsafe_char`]), cut at a character boundary.
pub fn clean_text(line: &str) -> String {
    let mut out = String::with_capacity(line.len().min(MAX_TEXT_LEN));
    for character in line.chars() {
        let character = if is_unsafe_char(character) {
            ' '
        } else {
            character
        };
        if out.len() + character.len_utf8() > MAX_TEXT_LEN {
            break;
        }
        out.push(character);
    }
    out
}

fn put_text8(out: &mut Vec<u8>, text: &str) {
    out.push(text.len().min(255) as u8);
    out.extend_from_slice(&text.as_bytes()[..text.len().min(255)]);
}

impl Stored {
    /// Builds a stored record from a decoded client record.
    pub fn from_owned(seq: u64, time_ms: u64, service: &str, record: OwnedRecord) -> Self {
        Self {
            seq,
            time_ms,
            kind: SourceKind::Service,
            service: service.to_owned(),
            level: record.level,
            key: record.key,
            fields: record.fields,
            text: String::new(),
        }
    }

    /// Encodes the payload.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(96 + self.text.len());
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&self.time_ms.to_le_bytes());
        out.push(self.kind.number());
        out.push(self.level.number());
        put_text8(&mut out, &self.service);
        put_text8(&mut out, &self.key);
        out.push(self.fields.len().min(MAX_FIELDS) as u8);
        for (name, value) in self.fields.iter().take(MAX_FIELDS) {
            put_text8(&mut out, name);
            match value {
                OwnedValue::Count(n) => {
                    out.push(0);
                    out.extend_from_slice(&n.to_le_bytes());
                }
                OwnedValue::Signed(n) => {
                    out.push(1);
                    out.extend_from_slice(&n.to_le_bytes());
                }
                OwnedValue::Flag(flag) => out.extend_from_slice(&[2, u8::from(*flag)]),
                OwnedValue::Word(word) => {
                    out.push(3);
                    put_text8(&mut out, word);
                }
                OwnedValue::Id(id) => {
                    out.push(4);
                    put_text8(&mut out, id);
                }
            }
        }
        let text = &self.text.as_bytes()[..self.text.len().min(MAX_TEXT_LEN)];
        out.extend_from_slice(&(text.len() as u16).to_le_bytes());
        out.extend_from_slice(text);
        out
    }

    /// Decodes and validates a payload.
    pub fn decode(payload: &[u8]) -> Result<Self, BadPayload> {
        if payload.len() > MAX_PAYLOAD {
            return Err(BadPayload);
        }
        let mut reader = Reader { bytes: payload };
        let seq = u64::from_le_bytes(reader.array()?);
        let time_ms = u64::from_le_bytes(reader.array()?);
        let kind = SourceKind::from_number(reader.byte()?).ok_or(BadPayload)?;
        let level = Level::from_number(reader.byte()?).ok_or(BadPayload)?;
        let service = reader.text8()?;
        if !is_service_name(service) {
            return Err(BadPayload);
        }
        let key = reader.key()?;
        let count = usize::from(reader.byte()?);
        if count > MAX_FIELDS {
            return Err(BadPayload);
        }
        let mut fields = Vec::with_capacity(count);
        for _ in 0..count {
            let name = reader.key()?;
            fields.push((name, reader.value()?));
        }
        let text_len = usize::from(u16::from_le_bytes(reader.array()?));
        if text_len > MAX_TEXT_LEN {
            return Err(BadPayload);
        }
        let text = std::str::from_utf8(reader.take(text_len)?).map_err(|_| BadPayload)?;
        if !reader.bytes.is_empty() || (kind != SourceKind::Kernel && !text.is_empty()) {
            return Err(BadPayload);
        }
        Ok(Self {
            seq,
            time_ms,
            kind,
            service: service.to_owned(),
            level,
            key,
            fields,
            text: text.to_owned(),
        })
    }

    /// Approximate memory use of the record (for the ring's byte bound).
    pub fn weight(&self) -> usize {
        let fields: usize = self
            .fields
            .iter()
            .map(|(name, value)| {
                name.len()
                    + match value {
                        OwnedValue::Word(text) | OwnedValue::Id(text) => text.len() + 32,
                        _ => 16,
                    }
                    + 48
            })
            .sum();
        160 + self.service.len() + self.key.len() + self.text.len() + fields
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], BadPayload> {
        if self.bytes.len() < count {
            return Err(BadPayload);
        }
        let (head, tail) = self.bytes.split_at(count);
        self.bytes = tail;
        Ok(head)
    }

    fn byte(&mut self) -> Result<u8, BadPayload> {
        Ok(self.take(1)?[0])
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], BadPayload> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    fn text8(&mut self) -> Result<&'a str, BadPayload> {
        let len = usize::from(self.byte()?);
        std::str::from_utf8(self.take(len)?).map_err(|_| BadPayload)
    }

    fn key(&mut self) -> Result<String, BadPayload> {
        let text = self.text8()?;
        if !is_key(text) {
            return Err(BadPayload);
        }
        Ok(text.to_owned())
    }

    fn value(&mut self) -> Result<OwnedValue, BadPayload> {
        Ok(match self.byte()? {
            0 => OwnedValue::Count(u64::from_le_bytes(self.array()?)),
            1 => OwnedValue::Signed(i64::from_le_bytes(self.array()?)),
            2 => match self.byte()? {
                0 => OwnedValue::Flag(false),
                1 => OwnedValue::Flag(true),
                _ => return Err(BadPayload),
            },
            3 => OwnedValue::Word(self.key()?),
            4 => {
                let text = self.text8()?;
                if !ShortId::is_valid(text) {
                    return Err(BadPayload);
                }
                OwnedValue::Id(text.to_owned())
            }
            _ => return Err(BadPayload),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::sample;

    #[test]
    fn records_round_trip_including_kernel_text() {
        let record = sample(42);
        assert_eq!(Stored::decode(&record.encode()).unwrap(), record);
        let kernel = Stored {
            kind: SourceKind::Kernel,
            service: KERNEL_NAME.into(),
            key: "kernel.line".into(),
            fields: Vec::new(),
            text: "[kern] bereit nach 145 ms – ✓".into(),
            ..sample(1)
        };
        assert_eq!(Stored::decode(&kernel.encode()).unwrap(), kernel);
        assert!(kernel.weight() > 160 && record.weight() > kernel.weight());
    }

    #[test]
    fn every_truncation_and_trailing_byte_is_refused() {
        let payload = sample(7).encode();
        for cut in 0..payload.len() {
            assert_eq!(Stored::decode(&payload[..cut]), Err(BadPayload), "{cut}");
        }
        let mut longer = payload.clone();
        longer.push(0);
        assert_eq!(Stored::decode(&longer), Err(BadPayload));
        assert_eq!(Stored::decode(&vec![0; MAX_PAYLOAD + 1]), Err(BadPayload));
    }

    #[test]
    fn invalid_content_is_refused() {
        let good = sample(1);
        let service_text = Stored {
            text: "services have no text".into(),
            ..good.clone()
        };
        assert_eq!(Stored::decode(&service_text.encode()), Err(BadPayload));
        let bad_name = Stored {
            service: "Bad Name".into(),
            ..good.clone()
        };
        assert_eq!(Stored::decode(&bad_name.encode()), Err(BadPayload));
        let bad_key = Stored {
            key: "Not A Key".into(),
            ..good.clone()
        };
        assert_eq!(Stored::decode(&bad_key.encode()), Err(BadPayload));
        let bad_id = Stored {
            fields: vec![("p".into(), OwnedValue::Id("+963115550100".into()))],
            ..good.clone()
        };
        assert_eq!(Stored::decode(&bad_id.encode()), Err(BadPayload));
        let mut bytes = good.encode();
        bytes[16] = 9; // kind
        assert_eq!(Stored::decode(&bytes), Err(BadPayload));
        let mut bytes = good.encode();
        bytes[17] = 9; // level
        assert_eq!(Stored::decode(&bytes), Err(BadPayload));
    }

    #[test]
    fn kernel_lines_are_cleaned_and_bounded() {
        assert_eq!(clean_text("a\tb\u{1b}[0m\n"), "a b [0m ");
        let long = "ش".repeat(400);
        let cleaned = clean_text(&long);
        assert!(cleaned.len() <= MAX_TEXT_LEN && cleaned.chars().all(|c| c == 'ش'));
        assert_eq!(cleaned.len(), MAX_TEXT_LEN);
        assert_eq!(clean_text(""), "");
    }

    #[test]
    fn names_and_kinds() {
        for kind in [
            SourceKind::Service,
            SourceKind::Kernel,
            SourceKind::Internal,
        ] {
            assert_eq!(SourceKind::from_number(kind.number()), Some(kind));
            assert!(!kind.name().is_empty());
        }
        assert_eq!(SourceKind::from_number(3), None);
        assert!(is_service_name("system.settingsd") && is_service_name(KERNEL_NAME));
        assert!(is_service_name(INTERNAL_NAME));
        for bad in ["", "A", "a b", "a/b", &"a".repeat(65)] {
            assert!(!is_service_name(bad));
        }
        let owned = OwnedRecord {
            level: Level::Info,
            key: "k".into(),
            fields: vec![],
        };
        let stored = Stored::from_owned(3, 4, "app.x", owned);
        assert_eq!(
            (stored.seq, stored.time_ms, stored.kind),
            (3, 4, SourceKind::Service)
        );
    }
}
