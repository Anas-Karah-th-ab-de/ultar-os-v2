//! Contracts of the settings service (`settingsd`).

use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Value of one setting.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum SettingValue {
    /// On/off switch.
    Flag(bool),
    /// Whole number (brightness, timeout in seconds, …).
    Integer(i64),
    /// Fractional number (scale factor, …).
    Decimal(f64),
    /// Identifier or text typed by the user (language tag, device name).
    Text(String),
    /// Ordered list of identifiers or texts (appended in 0.7.0 for
    /// `settingsd`; older values keep their encoding).
    List(Vec<String>),
}

pub use super::settings_access::*;
pub use super::settings_ext::*;

/// `settings.get` – read one setting.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Get {
    /// Hierarchical setting key, e.g. `display.scale`.
    pub key: String,
}

/// Answer to [`Get`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct GetReply {
    /// The value, or `None` when the key is not set.
    pub value: Option<SettingValue>,
}

impl Message for Get {
    const VERSION: u16 = 1;
}

impl Message for GetReply {
    const VERSION: u16 = 1;
}

impl Call for Get {
    const SERVICE: &'static str = "settings";
    const METHOD: &'static str = "get";
    type Reply = GetReply;
}

/// `settings.set` – change one setting.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Set {
    /// Hierarchical setting key.
    pub key: String,
    /// New value.
    pub value: SettingValue,
}

/// Answer to [`Set`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetReply {
    /// `false` when the stored value already was the requested one.
    pub changed: bool,
}

impl Message for Set {
    const VERSION: u16 = 1;
}

impl Message for SetReply {
    const VERSION: u16 = 1;
}

impl Call for Set {
    const SERVICE: &'static str = "settings";
    const METHOD: &'static str = "set";
    type Reply = SetReply;
}

/// `settings.changed` – a setting has a new value.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Changed {
    /// Hierarchical setting key.
    pub key: String,
    /// The value now in effect.
    pub value: SettingValue,
}

impl Message for Changed {
    const VERSION: u16 = 1;
}

impl Topic for Changed {
    const TOPIC: &'static str = "settings.changed";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn every_value_kind_round_trips() {
        let values = [
            SettingValue::Flag(true),
            SettingValue::Integer(-40),
            SettingValue::Decimal(1.25),
            SettingValue::Text("ar-SY".into()),
            SettingValue::List(vec!["a".into(), "b".into()]),
        ];
        for value in values {
            let changed = Changed {
                key: "locale.language".into(),
                value: value.clone(),
            };
            assert_eq!(
                decode::<Changed>(&encode(&changed).unwrap()).unwrap(),
                changed
            );
            let set = Set {
                key: "k".into(),
                value,
            };
            assert_eq!(decode::<Set>(&encode(&set).unwrap()).unwrap(), set);
        }
    }

    #[test]
    fn get_and_replies_round_trip() {
        let get = Get {
            key: "display.scale".into(),
        };
        assert_eq!(decode::<Get>(&encode(&get).unwrap()).unwrap(), get);
        for value in [None, Some(SettingValue::Decimal(1.5))] {
            let reply = GetReply { value };
            assert_eq!(decode::<GetReply>(&encode(&reply).unwrap()).unwrap(), reply);
        }
        let reply = SetReply { changed: true };
        assert_eq!(decode::<SetReply>(&encode(&reply).unwrap()).unwrap(), reply);
    }
}
