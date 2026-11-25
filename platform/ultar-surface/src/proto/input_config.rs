//! Limits of the pointer and key-repeat configuration ([`Request::ConfigurePointer`],
//! [`Request::ConfigureKeyRepeat`]) and the keys that switch the input language
//! ([`Request::ConfigureKeyboard`]).

#[allow(unused_imports)] // for the links above
use super::Request;
use crate::wire::{Reader, WireError, Writer};

/// Slowest pointer speed of [`Request::ConfigurePointer`].
pub const MIN_POINTER_SPEED: i8 = -10;
/// Fastest pointer speed of [`Request::ConfigurePointer`].
pub const MAX_POINTER_SPEED: i8 = 10;

/// Longest delay before a held key repeats ([`Request::ConfigureKeyRepeat`]), in milliseconds.
pub const MAX_KEY_REPEAT_DELAY_MS: u16 = 2000;
/// Most repeats per second of a held key ([`Request::ConfigureKeyRepeat`]).
pub const MAX_KEY_REPEATS_PER_SECOND: u8 = 60;

/// Shortest double-click interval of [`Request::ConfigurePointer`], in milliseconds.
pub const MIN_DOUBLE_CLICK_MS: u16 = 200;
/// Longest double-click interval of [`Request::ConfigurePointer`], in milliseconds.
pub const MAX_DOUBLE_CLICK_MS: u16 = 1000;
/// Double-click interval when the request does not carry one (the settings schema's default).
pub const DEFAULT_DOUBLE_CLICK_MS: u16 = 400;

/// Edge of the pointer image (`pointer.size`; `accessibility.large_pointer` forces at least
/// [`PointerSize::Large`]). The factors scale the token `size.icon`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PointerSize {
    /// The token's own size.
    #[default]
    Normal,
    /// One and a half times the token's size.
    Large,
    /// Twice the token's size.
    Larger,
}

impl PointerSize {
    /// How many times the token's edge the pointer image is drawn with.
    pub fn factor(self) -> f64 {
        match self {
            PointerSize::Normal => 1.0,
            PointerSize::Large => 1.5,
            PointerSize::Larger => 2.0,
        }
    }

    /// The larger of two sizes (a large pointer never shrinks below `other`).
    pub fn at_least(self, other: PointerSize) -> PointerSize {
        if self.factor() >= other.factor() {
            self
        } else {
            other
        }
    }

    /// The byte on the wire.
    pub fn encode(self, w: &mut Writer) {
        w.u8(match self {
            PointerSize::Normal => 0,
            PointerSize::Large => 1,
            PointerSize::Larger => 2,
        });
    }

    /// Reads one byte; unknown sizes are refused.
    pub fn decode(r: &mut Reader<'_>) -> Result<PointerSize, WireError> {
        Ok(match r.u8()? {
            0 => PointerSize::Normal,
            1 => PointerSize::Large,
            2 => PointerSize::Larger,
            _ => return Err(WireError::BadValue),
        })
    }
}

/// The keys that switch the input language (`keyboard.switch_shortcut`,
/// [`Request::ConfigureKeyboard`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SwitchShortcut {
    /// Super+Space, at once.
    #[default]
    SuperSpace,
    /// Alt+Shift, released with no other key pressed meanwhile.
    AltShift,
    /// Ctrl+Shift, released with no other key pressed meanwhile.
    CtrlShift,
}

impl SwitchShortcut {
    /// The byte on the wire.
    pub fn encode(self, w: &mut Writer) {
        w.u8(match self {
            SwitchShortcut::SuperSpace => 0,
            SwitchShortcut::AltShift => 1,
            SwitchShortcut::CtrlShift => 2,
        });
    }

    /// Reads one byte; unknown combinations are refused.
    pub fn decode(r: &mut Reader<'_>) -> Result<SwitchShortcut, WireError> {
        Ok(match r.u8()? {
            0 => SwitchShortcut::SuperSpace,
            1 => SwitchShortcut::AltShift,
            2 => SwitchShortcut::CtrlShift,
            _ => return Err(WireError::BadValue),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_shortcuts_round_trip_and_unknown_ones_are_refused() {
        assert_eq!(SwitchShortcut::default(), SwitchShortcut::SuperSpace);
        for shortcut in [
            SwitchShortcut::SuperSpace,
            SwitchShortcut::AltShift,
            SwitchShortcut::CtrlShift,
        ] {
            let mut w = Writer::new(0);
            shortcut.encode(&mut w);
            let frame = w.finish().unwrap();
            let mut r = Reader::new(&frame[8..]);
            assert_eq!(SwitchShortcut::decode(&mut r).unwrap(), shortcut);
        }
        let mut r = Reader::new(&[3]);
        assert_eq!(SwitchShortcut::decode(&mut r), Err(WireError::BadValue));
    }

    #[test]
    fn sizes_scale_and_never_shrink_below_the_floor() {
        assert_eq!(PointerSize::default(), PointerSize::Normal);
        assert_eq!(PointerSize::Normal.factor(), 1.0);
        assert_eq!(PointerSize::Large.factor(), 1.5);
        assert_eq!(PointerSize::Larger.factor(), 2.0);
        assert_eq!(
            PointerSize::Normal.at_least(PointerSize::Large),
            PointerSize::Large
        );
        assert_eq!(
            PointerSize::Larger.at_least(PointerSize::Large),
            PointerSize::Larger
        );
        assert_eq!(
            PointerSize::Large.at_least(PointerSize::Normal),
            PointerSize::Large
        );
        for size in [PointerSize::Normal, PointerSize::Large, PointerSize::Larger] {
            let mut w = Writer::new(0);
            size.encode(&mut w);
            let frame = w.finish().unwrap();
            let mut r = Reader::new(&frame[8..]);
            assert_eq!(PointerSize::decode(&mut r).unwrap(), size);
        }
        let mut r = Reader::new(&[3]);
        assert_eq!(PointerSize::decode(&mut r), Err(WireError::BadValue));
    }
}
