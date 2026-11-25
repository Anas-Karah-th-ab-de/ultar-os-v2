//! Bounce keys (`accessibility.bounce_keys`, `Request::ConfigureKeyHelpers`): a key that
//! bounces re-triggers itself, so repeats of the same key within the interval are dropped.
//! The setting is a switch; the interval is fixed at 300 ms. A driver that reports no event
//! times is passed through unchanged (as with the key repeat).

use ultar_hal::KeyState;

/// The interval within which a repeat of the same key is dropped, in microseconds.
pub const BOUNCE_INTERVAL_US: u64 = 300_000;
/// Most keys remembered at once (a keyboard cannot hold more).
const CAP: usize = 8;

/// Bounce keys: the last accepted event of each key and the presses being dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BounceKeys {
    /// The setting is on.
    pub on: bool,
    /// Last accepted event time per key (code, microseconds).
    recent: [(u16, u64); CAP],
    /// Slots of `recent` in use.
    count: usize,
    /// Next slot to overwrite when full.
    next: usize,
    /// Keys whose current press was dropped (their release is dropped too).
    dropped: [u16; CAP],
    /// Slots of `dropped` in use.
    dropped_count: usize,
}

impl BounceKeys {
    /// Turns the setting on or off; off forgets what was remembered.
    pub fn set_on(&mut self, on: bool) {
        self.on = on;
        if !on {
            self.count = 0;
            self.next = 0;
            self.dropped_count = 0;
        }
    }

    fn last(&self, code: u16) -> Option<u64> {
        self.recent[..self.count]
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, t)| *t)
    }

    fn record(&mut self, code: u16, time: u64) {
        if let Some(slot) = self.recent[..self.count].iter_mut().find(|(c, _)| *c == code) {
            slot.1 = time;
            return;
        }
        self.recent[self.next] = (code, time);
        self.next = (self.next + 1) % CAP;
        self.count = (self.count + 1).min(CAP);
    }

    fn is_dropped(&self, code: u16) -> bool {
        self.dropped[..self.dropped_count].contains(&code)
    }

    fn mark_dropped(&mut self, code: u16) {
        if !self.is_dropped(code) && self.dropped_count < CAP {
            self.dropped[self.dropped_count] = code;
            self.dropped_count += 1;
        }
    }

    fn take_dropped(&mut self, code: u16) -> bool {
        match self.dropped[..self.dropped_count]
            .iter()
            .position(|&c| c == code)
        {
            Some(i) => {
                self.dropped_count -= 1;
                self.dropped[i] = self.dropped[self.dropped_count];
                true
            }
            None => false,
        }
    }

    /// True when the event is a bounce to drop. A dropped press takes its release with it;
    /// a repeat of a key whose press was accepted is dropped on its own.
    pub fn is_bounce(&mut self, code: u16, state: KeyState, time_us: u64) -> bool {
        if !self.on || time_us == 0 {
            return false;
        }
        match state {
            KeyState::Released => {
                if self.take_dropped(code) {
                    return true;
                }
                self.record(code, time_us);
                false
            }
            KeyState::Pressed | KeyState::Repeated => {
                if self.is_dropped(code) {
                    return true;
                }
                if let Some(last) = self.last(code) {
                    if time_us.saturating_sub(last) < BOUNCE_INTERVAL_US {
                        if state == KeyState::Pressed {
                            self.mark_dropped(code);
                        }
                        return true;
                    }
                }
                self.record(code, time_us);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(state: KeyState, ms: u64) -> (KeyState, u64) {
        (state, ms * 1000)
    }

    #[test]
    fn off_drops_nothing() {
        let mut b = BounceKeys::default();
        let (s, t) = at(KeyState::Pressed, 0);
        assert!(!b.is_bounce(30, s, t));
        let (s, t) = at(KeyState::Pressed, 10);
        assert!(!b.is_bounce(30, s, t));
    }

    #[test]
    fn a_quick_repress_is_dropped_with_its_release() {
        let mut b = BounceKeys {
            on: true,
            ..BounceKeys::default()
        };
        let (s, t) = at(KeyState::Pressed, 100);
        assert!(!b.is_bounce(30, s, t), "first press counts");
        let (s, t) = at(KeyState::Released, 150);
        assert!(!b.is_bounce(30, s, t));
        // A bounce 100 ms later is dropped, and so is its release.
        let (s, t) = at(KeyState::Pressed, 250);
        assert!(b.is_bounce(30, s, t));
        let (s, t) = at(KeyState::Released, 300);
        assert!(b.is_bounce(30, s, t));
        // After the interval the key counts again.
        let (s, t) = at(KeyState::Pressed, 600);
        assert!(!b.is_bounce(30, s, t));
    }

    #[test]
    fn repeats_within_the_interval_are_dropped() {
        let mut b = BounceKeys {
            on: true,
            ..BounceKeys::default()
        };
        let (s, t) = at(KeyState::Pressed, 1000);
        assert!(!b.is_bounce(30, s, t));
        let (s, t) = at(KeyState::Repeated, 1100);
        assert!(b.is_bounce(30, s, t));
        let (s, t) = at(KeyState::Repeated, 1400);
        assert!(!b.is_bounce(30, s, t), "300 ms after the press");
        let (s, t) = at(KeyState::Repeated, 1500);
        assert!(b.is_bounce(30, s, t), "within 300 ms of the last accepted");
        // The release of an accepted press is not dropped.
        let (s, t) = at(KeyState::Released, 1800);
        assert!(!b.is_bounce(30, s, t));
    }

    #[test]
    fn another_key_is_independent_and_no_times_pass_through() {
        let mut b = BounceKeys {
            on: true,
            ..BounceKeys::default()
        };
        let (s, t) = at(KeyState::Pressed, 0);
        assert!(!b.is_bounce(30, s, t));
        let (s, t) = at(KeyState::Pressed, 50);
        assert!(!b.is_bounce(31, s, t), "a different key counts");
        // A driver without event times is passed through.
        assert!(!b.is_bounce(30, KeyState::Pressed, 0));
        assert!(!b.is_bounce(30, KeyState::Repeated, 0));
    }

    #[test]
    fn turning_it_off_forgets_the_interval() {
        let mut b = BounceKeys {
            on: true,
            ..BounceKeys::default()
        };
        let (s, t) = at(KeyState::Pressed, 0);
        assert!(!b.is_bounce(30, s, t));
        b.set_on(false);
        let (s, t) = at(KeyState::Pressed, 10);
        assert!(!b.is_bounce(30, s, t));
    }
}
