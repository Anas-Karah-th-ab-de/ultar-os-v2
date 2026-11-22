//! The typed API layer: contracts are Rust types, not ad-hoc byte strings.
//!
//! * [`Message`] – anything that travels on the bus, with a contract version.
//! * [`Topic`] – a message published on a fixed topic.
//! * [`Call`] – a request to a service method with its reply type.
//!
//! User-visible strings never travel as finished text: messages carry a
//! [`Text`], i.e. an i18n key plus arguments, and the receiver renders it in
//! the user's language (Arabic first).

use crate::error::BusError;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// A value with a versioned wire contract.
///
/// Bump [`Message::VERSION`] whenever the serialised layout changes; both
/// sides refuse messages whose version differs from the one they were built
/// with instead of misreading them.
pub trait Message: Serialize + DeserializeOwned {
    /// Contract version of the serialised layout.
    const VERSION: u16;
}

/// A message that is published on one fixed topic.
pub trait Topic: Message {
    /// Hierarchical topic name, e.g. `power.state`.
    const TOPIC: &'static str;
    /// Whether the broker keeps the last value for late subscribers.
    const RETAINED: bool = false;
}

/// A request to a service method.
pub trait Call: Message {
    /// Name of the service that implements the method.
    const SERVICE: &'static str;
    /// Method name (one name segment).
    const METHOD: &'static str;
    /// What the service answers.
    type Reply: Message;
}

/// The empty message, e.g. as reply of calls that return nothing.
impl Message for () {
    const VERSION: u16 = 1;
}

/// Serialises a value into the bus payload format.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, BusError> {
    postcard::to_allocvec(value).map_err(|_| BusError::Encode)
}

/// Deserialises a bus payload.
pub fn decode<T: DeserializeOwned>(payload: &[u8]) -> Result<T, BusError> {
    postcard::from_bytes(payload).map_err(|_| BusError::Malformed)
}

/// Deserialises a payload after checking its contract version.
pub fn decode_versioned<T: Message>(version: u16, payload: &[u8]) -> Result<T, BusError> {
    if version != T::VERSION {
        return Err(BusError::MessageVersion {
            expected: T::VERSION,
            found: version,
        });
    }
    decode(payload)
}

/// One argument of a translatable text.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum TextArg {
    /// Text inserted verbatim (names, numbers typed by a user, file names).
    Verbatim(String),
    /// A number the renderer formats for the user's locale.
    Number(i64),
    /// Another i18n key that is translated before it is inserted.
    Key(String),
}

/// A user-visible string as i18n key plus arguments – never finished text.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Text {
    /// Key in the i18n catalogue, e.g. `notify.call.incoming`.
    pub key: String,
    /// Arguments in the order the catalogue entry expects them.
    pub args: Vec<TextArg>,
}

impl Text {
    /// A text without arguments.
    pub fn key(key: &str) -> Self {
        Self {
            key: key.to_owned(),
            args: Vec::new(),
        }
    }

    /// Appends an argument.
    pub fn with(mut self, arg: TextArg) -> Self {
        self.args.push(arg);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Sample {
        value: u32,
    }

    impl Message for Sample {
        const VERSION: u16 = 4;
    }

    #[test]
    fn encode_decode_round_trip() {
        let bytes = encode(&Sample { value: 9 }).unwrap();
        assert_eq!(decode::<Sample>(&bytes).unwrap(), Sample { value: 9 });
        assert!(matches!(decode::<Sample>(&[]), Err(BusError::Malformed)));
    }

    #[test]
    fn versions_are_checked_before_decoding() {
        let bytes = encode(&Sample { value: 1 }).unwrap();
        assert_eq!(
            decode_versioned::<Sample>(4, &bytes).unwrap(),
            Sample { value: 1 }
        );
        assert!(matches!(
            decode_versioned::<Sample>(5, &bytes),
            Err(BusError::MessageVersion {
                expected: 4,
                found: 5
            })
        ));
    }

    #[test]
    fn unit_is_a_message() {
        let bytes = encode(&()).unwrap();
        assert!(bytes.is_empty());
        decode_versioned::<()>(<() as Message>::VERSION, &bytes).unwrap();
    }

    #[test]
    fn text_builder_keeps_argument_order() {
        let text = Text::key("notify.sms.from")
            .with(TextArg::Verbatim("Anas".into()))
            .with(TextArg::Number(3))
            .with(TextArg::Key("unit.messages".into()));
        assert_eq!(text.key, "notify.sms.from");
        assert_eq!(text.args.len(), 3);
        let bytes = encode(&text).unwrap();
        assert_eq!(decode::<Text>(&bytes).unwrap(), text);
        assert_eq!(Text::default().key, "");
    }
}
