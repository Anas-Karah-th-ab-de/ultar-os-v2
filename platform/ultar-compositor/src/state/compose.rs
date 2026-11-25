//! The compose key (`keyboard.compose_key`, `Request::ConfigureKeyHelpers`): the chosen key
//! starts a two-key sequence; the table (`compose_table.rs`) maps it to a character that is
//! delivered as text to the keyboard's target. Pressing the compose key again cancels; a
//! sequence the table does not know produces nothing. While the compose key is Caps Lock it
//! no longer toggles caps; while it is Alt Gr it no longer acts as a modifier.

use super::State;
use crate::keymap::{self, Output as KeyOutput};
use ultar_hal::KeyState;
use ultar_surface::proto::{ComposeKey, Event, Modifiers};

/// Most codes whose release is swallowed at once (the compose key and two sequence keys).
const CAP: usize = 4;

/// The compose key and the sequence being collected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Compose {
    /// The chosen key ([`ComposeKey::None`] = off).
    pub key: ComposeKey,
    /// A sequence is being collected.
    active: bool,
    /// The characters of the sequence so far.
    seq: [char; 2],
    /// Characters in `seq`.
    seq_len: usize,
    /// Codes whose release (and repeat) is swallowed.
    swallowed: [u16; CAP],
    /// Slots of `swallowed` in use.
    swallowed_count: usize,
}

impl Compose {
    /// Chooses the compose key; any sequence in progress is cancelled.
    pub fn set_key(&mut self, key: ComposeKey) {
        self.key = key;
        self.reset();
    }

    fn reset(&mut self) {
        self.active = false;
        self.seq_len = 0;
        self.swallowed_count = 0;
    }

    /// The Linux code of the chosen key.
    fn code(&self) -> Option<u16> {
        match self.key {
            ComposeKey::None => None,
            ComposeKey::AltGr => Some(100),
            ComposeKey::Menu => Some(139),
            ComposeKey::CapsLock => Some(58),
        }
    }

    fn swallow(&mut self, code: u16) {
        if !self.is_swallowed(code) && self.swallowed_count < CAP {
            self.swallowed[self.swallowed_count] = code;
            self.swallowed_count += 1;
        }
    }

    fn is_swallowed(&self, code: u16) -> bool {
        self.swallowed[..self.swallowed_count].contains(&code)
    }

    fn take_swallowed(&mut self, code: u16) -> bool {
        match self.swallowed[..self.swallowed_count]
            .iter()
            .position(|&c| c == code)
        {
            Some(i) => {
                self.swallowed_count -= 1;
                self.swallowed[i] = self.swallowed[self.swallowed_count];
                true
            }
            None => false,
        }
    }
}

impl State {
    /// The compose key and its sequence. True if the key was consumed (not delivered).
    pub(super) fn compose_event(&mut self, code: u16, state: KeyState) -> bool {
        if self.keyboard.helpers.compose.key == ComposeKey::None {
            return false;
        }
        match state {
            KeyState::Released => self.keyboard.helpers.compose.take_swallowed(code),
            KeyState::Repeated => self.keyboard.helpers.compose.is_swallowed(code),
            KeyState::Pressed => self.compose_pressed(code),
        }
    }

    fn compose_pressed(&mut self, code: u16) -> bool {
        if Some(code) == self.keyboard.helpers.compose.code() {
            let c = &mut self.keyboard.helpers.compose;
            if c.active {
                c.reset();
            } else {
                c.active = true;
                c.seq_len = 0;
            }
            c.swallow(code);
            return true;
        }
        if !self.keyboard.helpers.compose.active {
            return false;
        }
        let output = keymap::translate(
            self.key_layout(),
            code,
            Modifiers(self.keyboard.modifiers),
            self.keyboard.caps_lock,
        );
        let ch = match output {
            KeyOutput::Char(c) => Some(c),
            KeyOutput::Text(t) => t.chars().next(),
            KeyOutput::None => None,
        };
        let Some(ch) = ch else {
            self.keyboard.helpers.compose.reset();
            return true;
        };
        let result = {
            let c = &mut self.keyboard.helpers.compose;
            c.swallow(code);
            c.seq[c.seq_len] = ch;
            c.seq_len += 1;
            if c.seq_len == 2 {
                let r = super::compose_table::lookup(c.seq[0], c.seq[1]);
                c.active = false;
                c.seq_len = 0;
                r
            } else {
                None
            }
        };
        if let Some(text) = result {
            self.compose_deliver(text);
        }
        true
    }

    fn compose_deliver(&mut self, text: &str) {
        if let Some(target) = self.text_target() {
            self.send(
                target.client,
                Event::Text {
                    surface: target.surface,
                    text: text.to_owned(),
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_names_a_key_and_none_is_off() {
        let mut c = Compose::default();
        assert_eq!(c.key, ComposeKey::None);
        assert_eq!(c.code(), None);
        c.set_key(ComposeKey::AltGr);
        assert_eq!(c.code(), Some(100));
        c.set_key(ComposeKey::Menu);
        assert_eq!(c.code(), Some(139));
        c.set_key(ComposeKey::CapsLock);
        assert_eq!(c.code(), Some(58));
    }

    #[test]
    fn swallowed_releases_are_taken_once() {
        let mut c = Compose::default();
        c.swallow(139);
        c.swallow(40);
        assert!(c.is_swallowed(139));
        assert!(c.take_swallowed(139));
        assert!(!c.take_swallowed(139), "once");
        assert!(c.take_swallowed(40));
        assert!(!c.is_swallowed(40));
    }

    #[test]
    fn choosing_a_key_cancels_a_sequence() {
        let mut c = Compose::default();
        c.set_key(ComposeKey::Menu);
        c.active = true;
        c.seq_len = 1;
        c.swallow(139);
        c.set_key(ComposeKey::AltGr);
        assert!(!c.active);
        assert_eq!(c.seq_len, 0);
        assert!(!c.is_swallowed(139));
    }
}
