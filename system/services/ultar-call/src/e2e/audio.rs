//! Voice of a call: 16 kHz mono, 20 ms frames, IMA ADPCM (4 bits per sample, 64 kbit/s,
//! pure Rust and tiny), a jitter buffer that puts frames back in order and fills a lost one
//! with a quieter copy of the last, and conversion to and from the sound card's 48 kHz stereo.

use std::collections::BTreeMap;

/// Samples per second of the call's voice.
pub const RATE: u32 = 16_000;
/// Samples of one frame (20 ms).
pub const FRAME: usize = 320;
/// Bytes of one encoded frame: predictor (2), step index (1), 160 bytes of nibbles.
pub const CODED: usize = 3 + FRAME / 2;
/// Frames held before playing (60 ms against jitter).
const DEPTH: usize = 3;
/// Frames the buffer keeps at most (beyond that the oldest go: the voice must not lag).
const MAX_HELD: usize = 12;

const STEPS: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
const INDEX: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

#[derive(Clone, Copy, Default)]
struct Adpcm {
    pred: i32,
    index: i32,
}

impl Adpcm {
    fn step(&mut self, nibble: u8) -> i16 {
        let step = STEPS[self.index as usize];
        let mut diff = step >> 3;
        if nibble & 1 != 0 {
            diff += step >> 2;
        }
        if nibble & 2 != 0 {
            diff += step >> 1;
        }
        if nibble & 4 != 0 {
            diff += step;
        }
        self.pred += if nibble & 8 != 0 { -diff } else { diff };
        self.pred = self.pred.clamp(-32768, 32767);
        self.index = (self.index + INDEX[usize::from(nibble & 7)]).clamp(0, 88);
        self.pred as i16
    }

    fn nibble(&mut self, sample: i16) -> u8 {
        let step = STEPS[self.index as usize];
        let mut diff = i32::from(sample) - self.pred;
        let mut n = 0u8;
        if diff < 0 {
            n = 8;
            diff = -diff;
        }
        let mut s = step;
        for bit in [4u8, 2, 1] {
            if diff >= s {
                n |= bit;
                diff -= s;
            }
            s >>= 1;
        }
        self.step(n);
        n
    }
}

/// Encodes one frame of [`FRAME`] samples (fewer are padded with silence). Every frame stands
/// alone (a lost one does not spoil the next); its start step is the one that fits best.
pub fn encode(samples: &[i16]) -> Vec<u8> {
    (0..89)
        .step_by(4)
        .map(|index| encode_from(samples, index))
        .min_by_key(|c| error(samples, c))
        .unwrap_or_default()
}

fn error(samples: &[i16], coded: &[u8]) -> i64 {
    let get = |i: usize| i64::from(samples.get(i).copied().unwrap_or(0));
    decode(coded)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, s)| (get(i) - i64::from(*s)).pow(2))
        .sum()
}

fn encode_from(samples: &[i16], index: i32) -> Vec<u8> {
    let mut st = Adpcm {
        pred: i32::from(samples.first().copied().unwrap_or(0)),
        index,
    };
    let mut out = Vec::with_capacity(CODED);
    out.extend_from_slice(&(st.pred as i16).to_le_bytes());
    out.push(st.index as u8);
    let get = |i: usize| samples.get(i).copied().unwrap_or(0);
    for i in (0..FRAME).step_by(2) {
        let lo = st.nibble(get(i));
        let hi = st.nibble(get(i + 1));
        out.push(lo | (hi << 4));
    }
    out
}

/// Decodes one frame; `None` for bytes of the wrong length.
pub fn decode(b: &[u8]) -> Option<Vec<i16>> {
    if b.len() != CODED {
        return None;
    }
    let mut st = Adpcm {
        pred: i32::from(i16::from_le_bytes([b[0], b[1]])),
        index: i32::from(b[2].min(88)),
    };
    let mut out = Vec::with_capacity(FRAME);
    for byte in &b[3..] {
        out.push(st.step(byte & 0x0F));
        out.push(st.step(byte >> 4));
    }
    Some(out)
}

/// 48 kHz stereo (interleaved) → 16 kHz mono: both channels and three samples averaged.
pub fn from_card(stereo48: &[i16]) -> Vec<i16> {
    stereo48
        .chunks_exact(6)
        .map(|c| (c.iter().map(|s| i32::from(*s)).sum::<i32>() / 6) as i16)
        .collect()
}

/// 16 kHz mono → 48 kHz stereo (interleaved), linear in between.
pub fn to_card(mono16: &[i16]) -> Vec<i16> {
    let mut out = Vec::with_capacity(mono16.len() * 6);
    for (i, s) in mono16.iter().enumerate() {
        let next = i32::from(*mono16.get(i + 1).unwrap_or(s));
        let a = i32::from(*s);
        for k in 0..3 {
            let v = (a + (next - a) * k / 3) as i16;
            out.push(v);
            out.push(v);
        }
    }
    out
}

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

    /// The next 20 ms to play (silence until enough is buffered; a lost frame is replaced by
    /// the last one at half volume).
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
#[path = "audio_tests.rs"]
mod tests;
