//! Slow keys (`accessibility.slow_keys`, `Request::ConfigureKeyHelpers`): a key counts only
//! after being held for the interval. The setting is a switch; the interval is fixed at
//! 300 ms. The press is held back until the loop's tick finds it due; a release before that
//! cancels it (the key never counted). This works whatever the driver's own repeat does.

use ultar_hal::KeyState;

/// How long a key must be held before it counts, in milliseconds.
pub const SLOW_KEYS_MS: u64 = 300;
/// Most keys held back at once (a keyboard cannot hold more).
const CAP: usize = 8;

/// Slow keys: the presses waiting to count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SlowKeys {
    /// The setting is on.
    pub on: bool,
    /// Presses waiting to count (code, due time in milliseconds).
    pending: [(u16, u64); CAP],
    /// Slots of `pending` in use.
    count: usize,
}

impl SlowKeys {
    /// Turns the setting on or off. The waiting presses are kept so the caller can deliver
    /// them ([`SlowKeys::drain`]) when the setting turns off mid-hold.
    pub fn set_on(&mut self, on: bool) {
        self.on = on;
    }

    fn position(&self, code: u16) -> Option<usize> {
        self.pending[..self.count]
            .iter()
            .position(|(c, _)| *c == code)
    }

    fn remove(&mut self, i: usize) {
        self.count -= 1;
        self.pending[i] = self.pending[self.count];
    }

    /// Takes a key event; true when it is held back (not delivered now). A press starts the
    /// wait; a release before the key counted cancels it (and is held back too); repeats
    /// while waiting are held back.
    pub fn intercept(&mut self, code: u16, state: KeyState, now_ms: u64) -> bool {
        if !self.on {
            return false;
        }
        match state {
            KeyState::Pressed => {
                if self.position(code).is_none() && self.count < CAP {
                    self.pending[self.count] = (code, now_ms.saturating_add(SLOW_KEYS_MS));
                    self.count += 1;
                }
                true
            }
            KeyState::Released => match self.position(code) {
                Some(i) => {
                    self.remove(i);
                    true
                }
                None => false,
            },
            KeyState::Repeated => self.position(code).is_some(),
        }
    }

    /// The presses held long enough to count (a release would have removed them, so they are
    /// still held). They are no longer pending.
    pub fn take_due(&mut self, now_ms: u64) -> Vec<u16> {
        let mut due = Vec::new();
        let mut i = 0;
        while i < self.count {
            if self.pending[i].1 <= now_ms {
                due.push(self.pending[i].0);
                self.remove(i);
            } else {
                i += 1;
            }
        }
        due
    }

    /// Earliest time a pending press counts (for the loop's timeout).
    pub fn next_due(&self) -> Option<u64> {
        self.pending[..self.count].iter().map(|(_, t)| *t).min()
    }

    /// All pending presses, forgetting them (the setting turned off mid-hold).
    pub fn drain(&mut self) -> Vec<u16> {
        let codes = self.pending[..self.count].iter().map(|(c, _)| *c).collect();
        self.count = 0;
        codes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> SlowKeys {
        SlowKeys {
            on: true,
            ..SlowKeys::default()
        }
    }

    #[test]
    fn off_holds_nothing_back() {
        let mut s = SlowKeys::default();
        assert!(!s.intercept(30, KeyState::Pressed, 0));
        assert!(!s.intercept(30, KeyState::Released, 10));
        assert!(s.take_due(1000).is_empty());
    }

    #[test]
    fn a_short_press_never_counts() {
        let mut s = on();
        assert!(s.intercept(30, KeyState::Pressed, 0), "press held back");
        assert_eq!(s.next_due(), Some(SLOW_KEYS_MS));
        assert!(s.take_due(100).is_empty(), "not held long enough");
        assert!(
            s.intercept(30, KeyState::Released, 100),
            "release held back"
        );
        assert!(s.take_due(1000).is_empty(), "cancelled");
        assert_eq!(s.next_due(), None);
    }

    #[test]
    fn a_held_key_counts_after_the_interval() {
        let mut s = on();
        assert!(s.intercept(30, KeyState::Pressed, 0));
        assert!(
            s.intercept(30, KeyState::Repeated, 100),
            "repeat while waiting"
        );
        assert!(s.take_due(299).is_empty());
        assert_eq!(s.take_due(300), vec![30], "counts at 300 ms");
        assert_eq!(s.next_due(), None);
        // After it counted, repeats and the release pass.
        assert!(!s.intercept(30, KeyState::Repeated, 350));
        assert!(!s.intercept(30, KeyState::Released, 400));
    }

    #[test]
    fn several_keys_wait_independently() {
        let mut s = on();
        s.intercept(30, KeyState::Pressed, 0);
        s.intercept(31, KeyState::Pressed, 100);
        assert_eq!(s.take_due(300), vec![30]);
        assert_eq!(s.next_due(), Some(400));
        assert_eq!(s.take_due(400), vec![31]);
    }

    #[test]
    fn turning_it_off_drains_the_waiting_presses() {
        let mut s = on();
        s.intercept(30, KeyState::Pressed, 0);
        s.set_on(false);
        assert_eq!(s.drain(), vec![30]);
        assert!(!s.intercept(31, KeyState::Pressed, 0));
    }
}
