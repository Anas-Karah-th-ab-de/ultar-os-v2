//! `emergency.number`: the number `call.emergency` dials. The call service reads it
//! from the settings service itself (at start and on `settings.changed`) so the lock
//! screen can start an emergency call without ever sending a number. Every value is
//! validated here; an invalid one falls back to the schema default, 112.

use ultar_bus::messages::settings::SettingValue;

/// The setting key.
pub const KEY: &str = "emergency.number";
/// The schema default – a number that works almost everywhere.
pub const DEFAULT: &str = "112";
/// Shortest emergency number (112, 911, 999 …).
const MIN: usize = 3;
/// Longest (the schema's `max_chars`).
const MAX: usize = 16;

/// An emergency number: an optional `+` then digits, 3–16 characters. Stricter than
/// the call contract's `check_number` (no letters, spaces or tones).
pub fn valid(text: &str) -> bool {
    let digits = text.strip_prefix('+').unwrap_or(text);
    (MIN..=MAX).contains(&text.len())
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
}

/// The number a settings value means: valid text, else the default.
pub fn of(value: &SettingValue) -> String {
    match value {
        SettingValue::Text(t) if valid(t) => t.clone(),
        _ => DEFAULT.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_numbers_stay_and_bad_ones_fall_back_to_112() {
        for good in ["112", "911", "999", "110", "+49110", "1234567890123456"] {
            assert!(valid(good), "{good}");
            assert_eq!(of(&SettingValue::Text(good.into())), good);
        }
        for bad in [
            "",
            "12",
            "12345678901234567",
            "11a",
            "+",
            "1 2",
            "112-1",
            "*112",
            "UOS-1",
        ] {
            assert!(!valid(bad), "{bad}");
            assert_eq!(of(&SettingValue::Text(bad.into())), DEFAULT);
        }
        assert_eq!(of(&SettingValue::Flag(true)), DEFAULT);
        assert_eq!(of(&SettingValue::Integer(112)), DEFAULT);
    }
}
