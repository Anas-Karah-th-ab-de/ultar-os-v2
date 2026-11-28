//! The voice of a call over the mesh link: PCM 16-bit, 48 kHz mono, 20 ms frames.
//!
//! No pure-Rust Opus is in the dependency tree, so the frames go as plain PCM on the local
//! network (one frame is [`FRAME_BYTES`] bytes, base64 on the wire) – honest and cheap on
//! a LAN, with a size check at both ends. A [`Jitter`] puts received frames back in order
//! and fills a lost one with a quieter copy of the last (packet loss concealment by repeat
//! and fade).

use std::collections::BTreeMap;

/// Samples per second of the link's voice.
pub const RATE: u32 = 48_000;
/// Samples of one frame: 20 ms.
pub const FRAME_SAMPLES: usize = 960;
/// Bytes of one frame: 16-bit mono.
pub const FRAME_BYTES: usize = FRAME_SAMPLES * 2;
/// Frames held before playing (60 ms against jitter).
const DEPTH: usize = 3;
/// Frames the buffer keeps at most (the voice must not lag).
const MAX_HELD: usize = 12;

/// `true` when `bytes` are exactly one PCM frame; anything else is refused.
pub fn is_frame(bytes: &[u8]) -> bool {
    bytes.len() == FRAME_BYTES
}

/// The samples of one frame; `None` when the bytes are no frame.
pub fn decode_frame(bytes: &[u8]) -> Option<Vec<i16>> {
    if !is_frame(bytes) {
        return None;
    }
    Some(
        bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect(),
    )
}

/// The bytes of one frame.
pub fn encode_frame(samples: &[i16]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .take(FRAME_BYTES)
        .collect()
}

/// Puts received frames back in order for playing; a lost frame is filled with the last one
/// at half volume, and each further lost frame with half again (repeat and fade).
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
                return vec![0; FRAME_SAMPLES];
            }
            self.started = true;
            self.next = self.frames.keys().next().copied();
        }
        let Some(n) = self.next else {
            return vec![0; FRAME_SAMPLES];
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
                vec![0; FRAME_SAMPLES]
            }
            None => {
                self.last.iter_mut().for_each(|s| *s /= 2);
                if self.last.is_empty() {
                    vec![0; FRAME_SAMPLES]
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

    fn frame(value: i16, len: usize) -> Vec<i16> {
        vec![value; len]
    }

    #[test]
    fn a_frame_has_a_fixed_size_and_round_trips() {
        assert!(is_frame(&vec![0u8; FRAME_BYTES]));
        assert!(!is_frame(&vec![0u8; FRAME_BYTES + 1]));
        assert!(!is_frame(&vec![0u8; FRAME_BYTES - 1]));
        let samples = frame(1234, FRAME_SAMPLES);
        let bytes = encode_frame(&samples);
        assert_eq!(bytes.len(), FRAME_BYTES);
        assert_eq!(decode_frame(&bytes).unwrap(), samples);
        assert!(decode_frame(&[0, 1, 2]).is_none());
    }

    #[test]
    fn frames_play_in_order_and_a_gap_fades() {
        let mut jitter = Jitter::default();
        // Silence until the buffer is deep enough.
        assert_eq!(jitter.take(), vec![0; FRAME_SAMPLES]);
        // A frame arrives out of order and is put back in place.
        jitter.put(7, frame(7, FRAME_SAMPLES));
        jitter.put(3, frame(3, FRAME_SAMPLES));
        jitter.put(4, frame(4, FRAME_SAMPLES));
        assert_eq!(jitter.take(), frame(3, FRAME_SAMPLES));
        assert_eq!(jitter.take(), frame(4, FRAME_SAMPLES));
        // Frames 5 and 6 are lost: the last one repeats and fades while 7 waits.
        assert_eq!(jitter.take(), frame(2, FRAME_SAMPLES));
        assert_eq!(jitter.take(), frame(1, FRAME_SAMPLES));
        assert_eq!(jitter.take(), frame(7, FRAME_SAMPLES));
    }

    #[test]
    fn a_late_frame_is_ignored() {
        let mut jitter = Jitter::default();
        for seq in 0..DEPTH + 1 {
            jitter.put(seq as u64, frame(seq as i16, FRAME_SAMPLES));
        }
        let _ = jitter.take();
        jitter.put(0, frame(9, FRAME_SAMPLES)); // already played over
        assert_eq!(jitter.take(), frame(1, FRAME_SAMPLES));
    }
}
