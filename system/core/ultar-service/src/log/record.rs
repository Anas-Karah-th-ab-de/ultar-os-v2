//! What a log record may contain – enforced by the types.
//!
//! A record is a **message key** plus a few **typed fields**. There is no
//! way to put a runtime string into a record:
//!
//! * the message key, field names and enum-like values are `&'static str`,
//!   i.e. text that is part of the program;
//! * numbers are numbers (counts, sizes, codes, durations);
//! * the only value built at run time is a [`ShortId`]: at most
//!   [`ShortId::MAX_LEN`] characters from `a-z 0-9 . _ : -`, starting with
//!   a letter and without a run of more than [`ShortId::MAX_DIGIT_RUN`]
//!   digits – enough for `app.phone`, `pci:01:00.0`, `wlan0`, too little
//!   for a name, a phone number, an address, a message body or a token.
//!
//! Names of people, numbers, addresses, message bodies and tokens can thus
//! not be logged by accident; trying does not compile:
//!
//! ```compile_fail
//! use ultar_service::log::{Level, Record};
//! let name = String::from("Mohamad");
//! let _ = Record::new(Level::Info, "user.login").word("name", &name);
//! ```
//!
//! ```compile_fail
//! use ultar_service::log::{Level, Record};
//! let body = format!("Dear {}", "Anas");
//! let _ = Record::new(Level::Info, &body);
//! ```
//!
//! ```compile_fail
//! use ultar_service::log::{Level, Record};
//! let number = String::from("+963 11 555 0100");
//! let _ = Record::new(Level::Info, "call.started").id("peer", number);
//! ```
//!
//! ```compile_fail
//! use ultar_service::log::Value;
//! let _: Value = "free text".into();
//! ```
//!
//! What compiles:
//!
//! ```
//! use ultar_service::log::{Level, Record, ShortId};
//! let peer = ShortId::new("app.phone").unwrap();
//! let record = Record::new(Level::Warn, "bus.refused")
//!     .word("op", "publish")
//!     .id("peer", peer)
//!     .count("code", 13);
//! assert_eq!(record.fields().len(), 3);
//! assert!(ShortId::new("+963115550100").is_err());
//! assert!(ShortId::new("Mohamad Anas").is_err());
//! ```
//!
//! (A program can still defeat this on purpose, e.g. by leaking a `String`
//! to get a `&'static str`. The types stop accidents, review stops intent.)

use std::fmt;

/// Severity of a record, lowest first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Level {
    /// Detail for developers.
    Debug,
    /// Normal operation.
    Info,
    /// Normal but noteworthy.
    Notice,
    /// Something was refused, dropped or limited.
    Warn,
    /// An operation failed.
    Error,
    /// The service cannot continue.
    Critical,
}

impl Level {
    /// All levels, lowest first.
    pub const ALL: [Level; 6] = [
        Level::Debug,
        Level::Info,
        Level::Notice,
        Level::Warn,
        Level::Error,
        Level::Critical,
    ];

    /// The wire number.
    pub const fn number(self) -> u8 {
        self as u8
    }

    /// The level with this wire number.
    pub const fn from_number(number: u8) -> Option<Self> {
        match number {
            0 => Some(Self::Debug),
            1 => Some(Self::Info),
            2 => Some(Self::Notice),
            3 => Some(Self::Warn),
            4 => Some(Self::Error),
            5 => Some(Self::Critical),
            _ => None,
        }
    }

    /// Stable lower-case name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Notice => "notice",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Critical => "critical",
        }
    }

    /// The level with this name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|level| level.name() == name)
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Longest message key, field name or word in bytes.
pub const MAX_KEY_LEN: usize = 48;
/// Most fields of one record.
pub const MAX_FIELDS: usize = 8;

/// Whether `text` is a key: 1–[`MAX_KEY_LEN`] characters from
/// `a-z 0-9 . _ -`, starting with a letter, without a run of more than
/// [`ShortId::MAX_DIGIT_RUN`] digits.
///
/// The digit rule matters on the receiving side: the types keep a
/// well-behaved client to text of the program, but the log service cannot
/// know whether a word on the wire was `&'static str`. Refusing long digit
/// runs keeps phone numbers, account numbers and the like out of words and
/// keys too (review round 3, R3-W1). What remains possible for a
/// non-conforming client is a short lower-case word – no free text.
pub fn is_key(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut run = 0;
    !bytes.is_empty()
        && bytes.len() <= MAX_KEY_LEN
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|b| {
            run = if b.is_ascii_digit() { run + 1 } else { 0 };
            run <= ShortId::MAX_DIGIT_RUN
                && (b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
        })
}

/// The identifier text was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InvalidId;

impl fmt::Display for InvalidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a short identifier")
    }
}

impl std::error::Error for InvalidId {}

/// A short technical identifier built at run time (see the module docs).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShortId {
    len: u8,
    bytes: [u8; ShortId::MAX_LEN],
}

impl ShortId {
    /// Longest identifier in bytes.
    pub const MAX_LEN: usize = 32;
    /// Longest run of digits (a phone number has more).
    pub const MAX_DIGIT_RUN: usize = 6;

    /// Checks the rules of the module documentation.
    pub fn is_valid(text: &str) -> bool {
        let bytes = text.as_bytes();
        if bytes.is_empty() || bytes.len() > Self::MAX_LEN || !bytes[0].is_ascii_lowercase() {
            return false;
        }
        let mut run = 0;
        for byte in bytes {
            let allowed = byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b':' | b'-');
            run = if byte.is_ascii_digit() { run + 1 } else { 0 };
            if !allowed || run > Self::MAX_DIGIT_RUN {
                return false;
            }
        }
        true
    }

    /// Builds an identifier; refuses anything outside the rules.
    pub fn new(text: &str) -> Result<Self, InvalidId> {
        if !Self::is_valid(text) {
            return Err(InvalidId);
        }
        let mut bytes = [0u8; Self::MAX_LEN];
        bytes[..text.len()].copy_from_slice(text.as_bytes());
        Ok(Self {
            len: text.len() as u8,
            bytes,
        })
    }

    /// The identifier text.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }
}

impl fmt::Debug for ShortId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ShortId({})", self.as_str())
    }
}

impl fmt::Display for ShortId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The value of a field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Value {
    /// A count, size, duration or other unsigned quantity.
    Count(u64),
    /// A signed quantity or code.
    Signed(i64),
    /// Yes or no.
    Flag(bool),
    /// One word out of a fixed vocabulary of the program.
    Word(&'static str),
    /// A short technical identifier.
    Id(ShortId),
}

/// One record as a service builds it. The service's name and the time are
/// not part of it: the log service adds both itself (the name from the
/// kernel credentials of the connection, so it cannot be forged).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Record {
    level: Level,
    key: &'static str,
    fields: [(&'static str, Value); MAX_FIELDS],
    len: u8,
    overflow: bool,
}

impl Record {
    /// A record with the message `key` and no fields.
    pub const fn new(level: Level, key: &'static str) -> Self {
        Self {
            level,
            key,
            fields: [("", Value::Flag(false)); MAX_FIELDS],
            len: 0,
            overflow: false,
        }
    }

    const fn with(mut self, name: &'static str, value: Value) -> Self {
        if (self.len as usize) < MAX_FIELDS {
            self.fields[self.len as usize] = (name, value);
            self.len += 1;
        } else {
            self.overflow = true;
        }
        self
    }

    /// Adds an unsigned quantity.
    pub const fn count(self, name: &'static str, value: u64) -> Self {
        self.with(name, Value::Count(value))
    }

    /// Adds a signed quantity or code.
    pub const fn signed(self, name: &'static str, value: i64) -> Self {
        self.with(name, Value::Signed(value))
    }

    /// Adds a yes/no value.
    pub const fn flag(self, name: &'static str, value: bool) -> Self {
        self.with(name, Value::Flag(value))
    }

    /// Adds one word out of the program's own vocabulary.
    pub const fn word(self, name: &'static str, value: &'static str) -> Self {
        self.with(name, Value::Word(value))
    }

    /// Adds a short technical identifier.
    pub const fn id(self, name: &'static str, value: ShortId) -> Self {
        self.with(name, Value::Id(value))
    }

    /// Adds the identifier `text` if it is one, otherwise the word
    /// `invalid` – for identifiers that come from outside (a peer's
    /// identity, a device address) and must never fail a log call.
    pub fn id_or_invalid(self, name: &'static str, text: &str) -> Self {
        match ShortId::new(text) {
            Ok(id) => self.id(name, id),
            Err(InvalidId) => self.word(name, "invalid"),
        }
    }

    /// Severity.
    pub const fn level(&self) -> Level {
        self.level
    }

    /// Message key.
    pub const fn key(&self) -> &'static str {
        self.key
    }

    /// The fields in the order they were added.
    pub fn fields(&self) -> &[(&'static str, Value)] {
        &self.fields[..usize::from(self.len)]
    }

    /// Whether more than [`MAX_FIELDS`] fields were added (the extra ones
    /// were left out; the encoder refuses such a record).
    pub const fn overflowed(&self) -> bool {
        self.overflow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_ordered_and_round_trip() {
        let mut previous = None;
        for level in Level::ALL {
            assert_eq!(Level::from_number(level.number()), Some(level));
            assert_eq!(Level::from_name(level.name()), Some(level));
            assert_eq!(level.to_string(), level.name());
            assert!(previous < Some(level));
            previous = Some(level);
        }
        assert_eq!(Level::from_number(6), None);
        assert_eq!(Level::from_name("WARN"), None);
    }

    #[test]
    fn keys_are_lower_case_dotted_identifiers() {
        for good in ["a", "bus.refused", "log.dropped", "x-1_y.z9"] {
            assert!(is_key(good), "{good}");
        }
        let long = "a".repeat(MAX_KEY_LEN + 1);
        for bad in [
            "",
            "Bus.refused",
            "1abc",
            ".a",
            "a b",
            "a\n",
            "ключ",
            "a=b",
            "phone.9631155501",
            "n0963115550100",
            long.as_str(),
        ] {
            assert!(!is_key(bad), "{bad:?}");
        }
        assert!(is_key("seq.123456.ok"));
        assert!(is_key(&"a".repeat(MAX_KEY_LEN)));
    }

    #[test]
    fn short_ids_accept_technical_names_only() {
        for good in [
            "app.phone",
            "system.settingsd",
            "pci:01:00.0",
            "wlan0",
            "usb:1c7a:0577",
            "nvme-turbo",
            "a123456",
        ] {
            assert_eq!(ShortId::new(good).unwrap().as_str(), good);
        }
        let long = "a".repeat(ShortId::MAX_LEN + 1);
        for bad in [
            "",
            "+963115550100",
            "0115550100",
            "a1234567",
            "tel:00963115550100",
            "Mohamad",
            "mohamad anas",
            "anas@example.org",
            "شارع",
            "a/b",
            "a\nb",
            long.as_str(),
        ] {
            assert_eq!(ShortId::new(bad), Err(InvalidId), "{bad:?}");
        }
        let id = ShortId::new("app.x").unwrap();
        assert_eq!(format!("{id} {id:?}"), "app.x ShortId(app.x)");
        assert!(!InvalidId.to_string().is_empty());
    }

    #[test]
    fn records_keep_their_fields_in_order_and_flag_overflow() {
        let id = ShortId::new("app.phone").unwrap();
        let record = Record::new(Level::Error, "a.b")
            .count("n", 1)
            .signed("s", -2)
            .flag("f", true)
            .word("w", "x")
            .id("i", id);
        assert_eq!((record.level(), record.key()), (Level::Error, "a.b"));
        assert_eq!(
            record.fields(),
            &[
                ("n", Value::Count(1)),
                ("s", Value::Signed(-2)),
                ("f", Value::Flag(true)),
                ("w", Value::Word("x")),
                ("i", Value::Id(id)),
            ]
        );
        assert!(!record.overflowed());
        let mut full = Record::new(Level::Info, "k");
        for _ in 0..MAX_FIELDS {
            full = full.count("n", 0);
        }
        assert!(!full.overflowed());
        let over = full.count("n", 0);
        assert!(over.overflowed());
        assert_eq!(over.fields().len(), MAX_FIELDS);
    }

    #[test]
    fn outside_identifiers_never_fail_a_log_call() {
        let ok = Record::new(Level::Info, "k").id_or_invalid("peer", "app.phone");
        assert!(matches!(ok.fields()[0].1, Value::Id(_)));
        let bad = Record::new(Level::Info, "k").id_or_invalid("peer", "Mr. Smith +49 170 1234567");
        assert_eq!(bad.fields()[0].1, Value::Word("invalid"));
    }
}
