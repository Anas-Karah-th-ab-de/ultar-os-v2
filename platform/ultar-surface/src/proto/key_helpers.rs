//! The keyboard helpers of the accessibility page and the compose key
//! ([`Request::ConfigureKeyHelpers`]): sticky, slow, bounce and mouse keys plus
//! the key that starts a compose sequence. The display service passes the
//! settings `accessibility.{sticky,slow,bounce,mouse}_keys` and
//! `keyboard.compose_key` on; the compositor's input path follows them.

#[allow(unused_imports)] // for the links above
use super::Request;
use crate::wire::{Reader, WireError, Writer};

/// The key that starts a compose sequence (`keyboard.compose_key`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ComposeKey {
    /// No compose key (the setting's default).
    #[default]
    None,
    /// The Alt Gr key.
    AltGr,
    /// The menu key.
    Menu,
    /// The Caps Lock key (it no longer toggles caps while it is the compose key).
    CapsLock,
}

impl ComposeKey {
    /// The settings word (`none`, `alt_gr`, `menu`, `caps_lock`); unknown words
    /// fall back to the default ([`ComposeKey::None`]).
    pub fn from_word(t: &str) -> ComposeKey {
        match t {
            "alt_gr" => ComposeKey::AltGr,
            "menu" => ComposeKey::Menu,
            "caps_lock" => ComposeKey::CapsLock,
            _ => ComposeKey::None,
        }
    }

    /// The settings word of this choice.
    pub fn word(self) -> &'static str {
        match self {
            ComposeKey::None => "none",
            ComposeKey::AltGr => "alt_gr",
            ComposeKey::Menu => "menu",
            ComposeKey::CapsLock => "caps_lock",
        }
    }

    /// The byte on the wire.
    pub fn encode(self, w: &mut Writer) {
        w.u8(match self {
            ComposeKey::None => 0,
            ComposeKey::AltGr => 1,
            ComposeKey::Menu => 2,
            ComposeKey::CapsLock => 3,
        });
    }

    /// Reads one byte; unknown choices are refused.
    pub fn decode(r: &mut Reader<'_>) -> Result<ComposeKey, WireError> {
        Ok(match r.u8()? {
            0 => ComposeKey::None,
            1 => ComposeKey::AltGr,
            2 => ComposeKey::Menu,
            3 => ComposeKey::CapsLock,
            _ => return Err(WireError::BadValue),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_round_trips_and_unknown_values_are_refused() {
        for key in [
            ComposeKey::None,
            ComposeKey::AltGr,
            ComposeKey::Menu,
            ComposeKey::CapsLock,
        ] {
            assert_eq!(ComposeKey::from_word(key.word()), key);
            let mut w = Writer::new(0);
            key.encode(&mut w);
            let frame = w.finish().unwrap();
            let mut r = Reader::new(&frame[8..]);
            assert_eq!(ComposeKey::decode(&mut r).unwrap(), key);
        }
        assert_eq!(ComposeKey::default(), ComposeKey::None);
        assert_eq!(ComposeKey::from_word("super"), ComposeKey::None);
        let mut r = Reader::new(&[4]);
        assert_eq!(ComposeKey::decode(&mut r), Err(WireError::BadValue));
    }

    #[test]
    fn the_request_round_trips_and_bad_bodies_are_refused() {
        let request = Request::ConfigureKeyHelpers {
            sticky_keys: true,
            slow_keys: false,
            bounce_keys: true,
            mouse_keys: false,
            compose_key: ComposeKey::Menu,
        };
        let frame = request.encode().unwrap();
        let op = u16::from_le_bytes([frame[4], frame[5]]);
        assert_eq!(op, 46);
        let body = &frame[8..];
        assert_eq!(body.len(), 5);
        assert_eq!(Request::decode(op, body).unwrap(), request);
        // Truncated and trailing bytes fail.
        assert!(Request::decode(op, &body[..4]).is_err());
        let mut longer = body.to_vec();
        longer.push(0);
        assert!(Request::decode(op, &longer).is_err());
        // Not a bool, and an unknown compose choice.
        assert!(Request::decode(46, &[2, 0, 0, 0, 0]).is_err());
        assert!(Request::decode(46, &[0, 0, 0, 0, 9]).is_err());
    }
}
