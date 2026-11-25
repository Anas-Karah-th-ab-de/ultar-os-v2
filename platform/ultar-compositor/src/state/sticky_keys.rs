//! Sticky keys (`accessibility.sticky_keys`, `Request::ConfigureKeyHelpers`): a modifier
//! pressed and released alone stays active for the next key; pressing it twice locks it
//! until it is pressed again. A modifier pressed together with another (a chord) behaves
//! normally and does not latch.

use ultar_hal::KeyState;

/// Sticky keys: which modifiers are latched (active for the next key) or locked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StickyKeys {
    /// The setting is on.
    pub on: bool,
    /// Modifiers active for the next key (bits of `Modifiers`).
    latched: u8,
    /// Modifiers active until pressed again (bits of `Modifiers`).
    locked: u8,
    /// Modifier bits physically held.
    held: u8,
    /// Modifiers that were part of a chord: they do not latch on release.
    chorded: u8,
}

impl StickyKeys {
    /// Turns the setting on or off; off forgets every latch and lock.
    pub fn set_on(&mut self, on: bool) {
        self.on = on;
        if !on {
            self.latched = 0;
            self.locked = 0;
            self.chorded = 0;
        }
    }

    /// The modifiers that stay active without being held (latched or locked).
    pub fn active(&self) -> u8 {
        self.latched | self.locked
    }

    /// Takes one key event (`bit` is its modifier bit, if any) and returns the extra
    /// modifier bits in effect for it. A non-modifier press consumes the latch (the bits
    /// apply to that key only); locks stay. Physical holding is tracked even while off, so
    /// turning the setting on mid-hold stays consistent.
    pub fn on_key(&mut self, bit: Option<u8>, state: KeyState) -> u8 {
        match (bit, state) {
            (Some(b), KeyState::Pressed) => {
                if self.held != 0 {
                    self.chorded |= self.held | b;
                }
                self.held |= b;
            }
            (Some(b), KeyState::Released) => self.held &= !b,
            (None, KeyState::Pressed) => self.chorded |= self.held,
            _ => {}
        }
        if !self.on {
            return 0;
        }
        match (bit, state) {
            (Some(b), KeyState::Pressed) => {
                if self.locked & b != 0 {
                    // Pressing a locked modifier unlocks it; this press does not latch.
                    self.locked &= !b;
                    self.chorded |= b;
                } else if self.latched & b != 0 {
                    // A second press of a latched modifier locks it.
                    self.latched &= !b;
                    self.locked |= b;
                }
            }
            (Some(b), KeyState::Released) => {
                if self.chorded & b == 0 && self.locked & b == 0 && self.latched & b == 0 {
                    self.latched |= b;
                }
                self.chorded &= !b;
            }
            (None, KeyState::Pressed) => {
                let take = self.latched;
                self.latched = 0;
                return take | self.locked;
            }
            _ => {}
        }
        self.latched | self.locked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_surface::proto::Modifiers;

    const SHIFT: Option<u8> = Some(Modifiers::SHIFT);
    const CTRL: Option<u8> = Some(Modifiers::CTRL);

    fn press(s: &mut StickyKeys, bit: Option<u8>) -> u8 {
        s.on_key(bit, KeyState::Pressed)
    }
    fn release(s: &mut StickyKeys, bit: Option<u8>) -> u8 {
        s.on_key(bit, KeyState::Released)
    }

    #[test]
    fn off_keeps_modifiers_normal() {
        let mut s = StickyKeys::default();
        assert_eq!(press(&mut s, SHIFT), 0);
        assert_eq!(release(&mut s, SHIFT), 0);
        assert_eq!(press(&mut s, None), 0);
        assert_eq!(s.active(), 0);
    }

    #[test]
    fn a_modifier_released_alone_latches_for_the_next_key_only() {
        let mut s = StickyKeys {
            on: true,
            ..StickyKeys::default()
        };
        press(&mut s, SHIFT);
        assert_eq!(release(&mut s, SHIFT), Modifiers::SHIFT, "stays active");
        assert_eq!(s.active(), Modifiers::SHIFT);
        // The next key sees Shift and consumes the latch.
        assert_eq!(press(&mut s, None), Modifiers::SHIFT);
        assert_eq!(s.active(), 0);
        assert_eq!(release(&mut s, None), 0);
        // The key after that is normal again.
        assert_eq!(press(&mut s, None), 0);
    }

    #[test]
    fn pressing_twice_locks_until_pressed_again() {
        let mut s = StickyKeys {
            on: true,
            ..StickyKeys::default()
        };
        press(&mut s, SHIFT);
        release(&mut s, SHIFT); // latched
        press(&mut s, SHIFT); // second press locks
        assert_eq!(release(&mut s, SHIFT), Modifiers::SHIFT);
        assert_eq!(s.active(), Modifiers::SHIFT);
        // A key consumes the latch but the lock stays.
        assert_eq!(press(&mut s, None), Modifiers::SHIFT);
        assert_eq!(s.active(), Modifiers::SHIFT);
        assert_eq!(press(&mut s, None), Modifiers::SHIFT);
        // Pressing the locked modifier again unlocks it.
        press(&mut s, SHIFT);
        assert_eq!(release(&mut s, SHIFT), 0);
        assert_eq!(s.active(), 0);
    }

    #[test]
    fn latched_modifiers_combine_and_chords_do_not_latch() {
        let mut s = StickyKeys {
            on: true,
            ..StickyKeys::default()
        };
        press(&mut s, SHIFT);
        release(&mut s, SHIFT);
        press(&mut s, CTRL);
        release(&mut s, CTRL);
        assert_eq!(s.active(), Modifiers::SHIFT | Modifiers::CTRL);
        assert_eq!(press(&mut s, None), Modifiers::SHIFT | Modifiers::CTRL);
        assert_eq!(s.active(), 0);
        // A real chord (Ctrl held while Shift presses) does not latch.
        press(&mut s, CTRL);
        press(&mut s, SHIFT);
        release(&mut s, SHIFT);
        release(&mut s, CTRL);
        assert_eq!(s.active(), 0);
    }

    #[test]
    fn turning_it_off_forgets_the_latch_and_lock() {
        let mut s = StickyKeys {
            on: true,
            ..StickyKeys::default()
        };
        press(&mut s, SHIFT);
        release(&mut s, SHIFT);
        press(&mut s, SHIFT);
        release(&mut s, SHIFT); // locked
        s.set_on(false);
        assert_eq!(s.active(), 0);
        assert_eq!(press(&mut s, None), 0);
    }
}
