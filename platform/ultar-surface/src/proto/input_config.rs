//! Limits of the pointer and key-repeat configuration ([`Request::ConfigurePointer`],
//! [`Request::ConfigureKeyRepeat`]).

#[allow(unused_imports)] // for the links above
use super::Request;

/// Slowest pointer speed of [`Request::ConfigurePointer`].
pub const MIN_POINTER_SPEED: i8 = -10;
/// Fastest pointer speed of [`Request::ConfigurePointer`].
pub const MAX_POINTER_SPEED: i8 = 10;

/// Longest delay before a held key repeats ([`Request::ConfigureKeyRepeat`]), in milliseconds.
pub const MAX_KEY_REPEAT_DELAY_MS: u16 = 2000;
/// Most repeats per second of a held key ([`Request::ConfigureKeyRepeat`]).
pub const MAX_KEY_REPEATS_PER_SECOND: u8 = 60;
