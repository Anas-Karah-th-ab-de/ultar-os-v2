//! The accessibility calls of the settings service for the sign-in screen (`login.access`,
//! DESKTOP-SPEC §5.3): a narrow right next to `settings.set`. A caller that may call these
//! methods (the lock screen, `system.lock`) can read and change only the keys under
//! [`ACCESSIBILITY_PREFIX`], with the types the schema declares; every other key is refused
//! (`NotPermitted`) before the schema is even asked.

use super::settings::{SetReply, SettingValue};
use crate::message::{Call, Message};
use serde::{Deserialize, Serialize};

/// The only keys these calls accept.
pub const ACCESSIBILITY_PREFIX: &str = "accessibility.";

/// Most accessibility settings answered by [`Accessibility`].
pub const MAX_ACCESSIBILITY: usize = 32;

/// `true` for a key the accessibility calls accept: the prefix, then one or more
/// dot-separated words of lower-case letters, digits and `_` (no empty word).
pub fn is_accessibility_key(key: &str) -> bool {
    let Some(rest) = key.strip_prefix(ACCESSIBILITY_PREFIX) else {
        return false;
    };
    key.len() <= 128
        && rest.split('.').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

/// `settings.set_accessibility` – change one setting under `accessibility.*`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct SetAccessibility {
    /// A key under [`ACCESSIBILITY_PREFIX`].
    pub key: String,
    /// New value, of the type the schema declares.
    pub value: SettingValue,
}

/// `settings.accessibility` – the current values of every `accessibility.*` setting.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Accessibility {}

/// Answer to [`Accessibility`]: (key, value) in schema order, at most
/// [`MAX_ACCESSIBILITY`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct AccessibilityReply {
    /// The settings.
    pub values: Vec<(String, SettingValue)>,
}

impl Message for SetAccessibility {
    const VERSION: u16 = 1;
}

impl Message for Accessibility {
    const VERSION: u16 = 1;
}

impl Message for AccessibilityReply {
    const VERSION: u16 = 1;
}

impl Call for SetAccessibility {
    const SERVICE: &'static str = "settings";
    const METHOD: &'static str = "set_accessibility";
    type Reply = SetReply;
}

impl Call for Accessibility {
    const SERVICE: &'static str = "settings";
    const METHOD: &'static str = "accessibility";
    type Reply = AccessibilityReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn only_accessibility_keys_pass() {
        for good in ["accessibility.large_text", "accessibility.a.b_2"] {
            assert!(is_accessibility_key(good), "{good}");
        }
        for bad in [
            "accessibility.",
            "accessibility",
            "appearance.theme",
            "accessibility..x",
            "accessibility.X",
            "accessibility.x/../y",
            "xaccessibility.large_text",
            " accessibility.large_text",
        ] {
            assert!(!is_accessibility_key(bad), "{bad}");
        }
        assert!(!is_accessibility_key(&format!(
            "accessibility.{}",
            "a".repeat(200)
        )));
    }

    #[test]
    fn round_trips() {
        let set = SetAccessibility {
            key: "accessibility.high_contrast".into(),
            value: SettingValue::Flag(true),
        };
        assert_eq!(
            decode::<SetAccessibility>(&encode(&set).unwrap()).unwrap(),
            set
        );
        let reply = AccessibilityReply {
            values: vec![("accessibility.large_text".into(), SettingValue::Flag(false))],
        };
        assert_eq!(
            decode::<AccessibilityReply>(&encode(&reply).unwrap()).unwrap(),
            reply
        );
    }
}
