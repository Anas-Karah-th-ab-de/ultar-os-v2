//! The jitter buffer of the mesh voice: puts received 20 ms frames back in order, holds
//! three of them (60 ms) before it starts and conceals a lost frame by repeating the last
//! one at half volume (and half again for every further lost one).
//!
//! The same rule as `ultar_mesh::audio_link::Jitter` and the 16 kHz buffer of
//! [`crate::e2e::audio`], for 48 kHz frames: this service does not link the mesh crate.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::mesh_tap::FRAME;
use std::collections::BTreeMap;

/// Frames held before playing (60 ms against jitter).
const DEPTH: usize = 3;
/// Frames the buffer keeps at most (the voice must not lag).
const MAX_HELD: usize = 12;

/// Puts received frames back in order for playing.
#[derive(Default)]
pub struct Jitter {
    frames: BTreeMap<u64, Vec<i16>>,
    next: Option<u64>,
    last: Vec<i16>,
    started: bool,
}

impl Jitter {
    /// A frame with its sequence number arrived.
    pub fn put(&mut self, seq: u64, frame: Vec<i16>) {
        if self.next.is_some_and(|n| seq < n) {
            return; // too late, already played over
        }
        self.frames.insert(seq, frame);
        while self.frames.len() > MAX_HELD {
            self.frames.pop_first();
            self.next = self.frames.keys().next().copied();
        }
    }

    /// The next 20 ms to play (silence until enough is buffered).
    pub fn take(&mut self) -> Vec<i16> {
        if !self.started {
            if self.frames.len() < DEPTH {
                return vec![0; FRAME];
            }
            self.started = true;
            self.next = self.frames.keys().next().copied();
        }
        let Some(n) = self.next else {
            return vec![0; FRAME];
        };
        self.next = Some(n + 1);
        match self.frames.remove(&n) {
            Some(f) => {
                self.last.clone_from(&f);
                f
            }
            None if self.frames.is_empty() => {
                self.started = false;
                self.next = None;
                vec![0; FRAME]
            }
            None => {
                self.last.iter_mut().for_each(|s| *s /= 2);
                if self.last.is_empty() {
                    vec![0; FRAME]
                } else {
                    self.last.clone()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(value: i16) -> Vec<i16> {
        vec![value; FRAME]
    }

    #[test]
    fn frames_play_in_order_a_gap_fades_and_a_late_frame_is_ignored() {
        let mut jitter = Jitter::default();
        assert_eq!(jitter.take(), frame(0), "silence until deep enough");
        jitter.put(7, frame(8));
        jitter.put(3, frame(40));
        jitter.put(4, frame(20));
        assert_eq!(jitter.take(), frame(40));
        assert_eq!(jitter.take(), frame(20));
        // 5 and 6 are lost: the last repeats at half, then half again, while 7 waits.
        assert_eq!(jitter.take(), frame(10));
        assert_eq!(jitter.take(), frame(5));
        assert_eq!(jitter.take(), frame(8));
        jitter.put(4, frame(99)); // already played over
        assert_eq!(jitter.take(), frame(0), "nothing left: silence, restart");
    }

    #[test]
    fn the_buffer_is_bounded() {
        let mut jitter = Jitter::default();
        for seq in 0..100u64 {
            jitter.put(seq, frame(1));
        }
        assert!(jitter.frames.len() <= MAX_HELD);
    }
}
