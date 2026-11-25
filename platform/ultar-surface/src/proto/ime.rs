//! Keys the on-screen keyboard sends to the focused field ([`super::Request::ImeKey`]):
//! only the editing keys a field needs; text goes as [`super::Request::ImeCommit`].

use crate::wire::{Reader, WireError, Writer};

/// A key of the input method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImeKey {
    /// Deletes the character before the cursor.
    Backspace,
    /// The field's action: new line, send, search.
    Enter,
}

impl ImeKey {
    /// The byte on the wire.
    pub fn encode(self, w: &mut Writer) {
        w.u8(match self {
            ImeKey::Backspace => 0,
            ImeKey::Enter => 1,
        });
    }

    /// Reads one byte; other keys are refused.
    pub fn decode(r: &mut Reader<'_>) -> Result<ImeKey, WireError> {
        Ok(match r.u8()? {
            0 => ImeKey::Backspace,
            1 => ImeKey::Enter,
            _ => return Err(WireError::BadValue),
        })
    }

    /// The Linux key code the field receives.
    pub fn code(self) -> u16 {
        match self {
            ImeKey::Backspace => 14,
            ImeKey::Enter => 28,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ime_keys_round_trip_and_others_are_refused() {
        for key in [ImeKey::Backspace, ImeKey::Enter] {
            let mut w = Writer::new(0);
            key.encode(&mut w);
            let frame = w.finish().unwrap();
            assert_eq!(ImeKey::decode(&mut Reader::new(&frame[8..])).unwrap(), key);
        }
        assert!(ImeKey::decode(&mut Reader::new(&[7])).is_err());
        assert_eq!(ImeKey::Backspace.code(), 14);
    }
}
