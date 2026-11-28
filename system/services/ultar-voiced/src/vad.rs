//! Voice activity: which 10 ms hops of a stream hold speech, and the utterances between
//! pauses. Energy based with a slowly following noise floor, so a quiet room and a noisy one
//! both work. Nothing here keeps audio longer than the utterance being cut.

use std::collections::VecDeque;

/// Samples per hop (10 ms at 16 kHz).
pub const HOP: usize = 160;
/// Speech must be this much louder than the noise floor.
const FLOOR_FACTOR: f32 = 3.0;
/// And at least this loud in absolute terms (root-mean-square, -1.0..1.0).
const ABS_MIN: f32 = 0.012;
/// Hops of pause that end an utterance (300 ms).
pub const PAUSE_HOPS: usize = 30;
/// Fewest speech hops of an utterance (150 ms): shorter ones are clicks.
const MIN_SPEECH_HOPS: usize = 15;
/// Hops kept before the first speech hop (100 ms).
const PRE_ROLL: usize = 10;
/// The noise floor never goes below this.
const FLOOR_MIN: f32 = 0.002;

/// Root-mean-square of a slice (`0.0` when empty).
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

/// One hop with its verdict.
#[derive(Clone, Debug, PartialEq)]
pub struct Hop {
    /// The hop holds speech.
    pub speech: bool,
    /// Its samples ([`HOP`] of them).
    pub samples: Vec<f32>,
}

/// Cuts a stream into hops and judges each.
#[derive(Clone, Debug)]
pub struct Vad {
    floor: f32,
    rest: Vec<f32>,
}

impl Default for Vad {
    fn default() -> Self {
        Self::new()
    }
}

impl Vad {
    /// A detector with a quiet-room floor.
    pub fn new() -> Self {
        Self {
            floor: 0.005,
            rest: Vec::new(),
        }
    }

    /// Forgets the buffered remainder (the floor stays: the room did not change).
    pub fn reset(&mut self) {
        self.rest.clear();
    }

    /// Feeds samples; returns every complete hop.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Hop> {
        self.rest.extend_from_slice(samples);
        let mut out = Vec::new();
        let whole = self.rest.len() / HOP * HOP;
        for chunk in self.rest[..whole].chunks_exact(HOP) {
            let level = rms(chunk);
            let speech = level > ABS_MIN && level > self.floor * FLOOR_FACTOR;
            if !speech {
                // The floor follows the quiet: slowly up, quickly down.
                let rate = if level < self.floor { 0.2 } else { 0.01 };
                self.floor = (self.floor + (level - self.floor) * rate).max(FLOOR_MIN);
            }
            out.push(Hop {
                speech,
                samples: chunk.to_vec(),
            });
        }
        self.rest.drain(..whole);
        out
    }
}

/// Cuts utterances out of a stream: speech followed by a pause of [`PAUSE_HOPS`].
#[derive(Clone, Debug)]
pub struct Segmenter {
    vad: Vad,
    pre: VecDeque<Vec<f32>>,
    active: Vec<f32>,
    speech_hops: usize,
    pause: usize,
    in_speech: bool,
    max_samples: usize,
    overflow: bool,
}

impl Segmenter {
    /// A segmenter that drops utterances longer than `max_samples`.
    pub fn new(max_samples: usize) -> Self {
        Self {
            vad: Vad::new(),
            pre: VecDeque::new(),
            active: Vec::new(),
            speech_hops: 0,
            pause: 0,
            in_speech: false,
            max_samples,
            overflow: false,
        }
    }

    /// Drops everything buffered (a new listening round).
    pub fn reset(&mut self) {
        self.vad.reset();
        self.pre.clear();
        self.active.clear();
        self.speech_hops = 0;
        self.pause = 0;
        self.in_speech = false;
        self.overflow = false;
    }

    /// What was heard after the last utterance ended (the start of whatever follows), taken
    /// out of the segmenter.
    pub fn take_rest(&mut self) -> Vec<f32> {
        let rest = if self.in_speech {
            std::mem::take(&mut self.active)
        } else {
            self.pre.iter().flatten().copied().collect()
        };
        self.reset();
        rest
    }

    /// Feeds samples; returns the utterances that ended in them.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        let mut out = Vec::new();
        for hop in self.vad.push(samples) {
            if hop.speech {
                if !self.in_speech {
                    self.in_speech = true;
                    self.active = self.pre.iter().flatten().copied().collect();
                }
                self.pause = 0;
                self.speech_hops += 1;
                self.active.extend_from_slice(&hop.samples);
            } else if self.in_speech {
                self.pause += 1;
                self.active.extend_from_slice(&hop.samples);
                if self.pause >= PAUSE_HOPS {
                    let long_enough = self.speech_hops >= MIN_SPEECH_HOPS && !self.overflow;
                    let done = std::mem::take(&mut self.active);
                    if long_enough {
                        out.push(done);
                    }
                    self.in_speech = false;
                    self.speech_hops = 0;
                    self.pause = 0;
                    self.overflow = false;
                    self.pre.clear();
                    continue;
                }
            }
            if self.in_speech && self.active.len() > self.max_samples {
                // Too long to be what is looked for: stop keeping it.
                self.overflow = true;
                self.active.clear();
            }
            if !self.in_speech {
                self.pre.push_back(hop.samples);
                if self.pre.len() > PRE_ROLL {
                    self.pre.pop_front();
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn tone(len: usize, level: f32) -> Vec<f32> {
        (0..len)
            .map(|i| (2.0 * PI * 300.0 * i as f32 / 16_000.0).sin() * level)
            .collect()
    }

    #[test]
    fn hops_are_judged_against_a_quiet_floor() {
        let mut vad = Vad::new();
        let quiet = vad.push(&vec![0.001; HOP * 20]);
        assert_eq!(quiet.len(), 20);
        assert!(quiet.iter().all(|h| !h.speech));
        let loud = vad.push(&tone(HOP * 5, 0.3));
        assert!(loud.iter().all(|h| h.speech));
        // a partial hop waits for the rest
        assert!(vad.push(&[0.0; HOP - 1]).is_empty());
        assert_eq!(vad.push(&[0.0; 1]).len(), 1);
    }

    #[test]
    fn an_utterance_ends_after_the_pause_and_clicks_are_dropped() {
        let mut seg = Segmenter::new(16_000 * 4);
        let mut stream = vec![0.0f32; 8_000];
        stream.extend(tone(8_000, 0.3));
        stream.extend(vec![0.0f32; PAUSE_HOPS * HOP + 800]);
        let heard = seg.push(&stream);
        assert_eq!(heard.len(), 1);
        assert!(heard[0].len() >= 8_000 && heard[0].len() < 8_000 + 3 * 16_000 / 10 + 4_000);
        // a click of 40 ms is not an utterance
        let mut click = tone(640, 0.5);
        click.extend(vec![0.0f32; PAUSE_HOPS * HOP + 800]);
        assert!(seg.push(&click).is_empty());
    }

    #[test]
    fn an_utterance_longer_than_the_limit_is_dropped() {
        let mut seg = Segmenter::new(16_000);
        let mut stream = tone(16_000 * 3, 0.3);
        stream.extend(vec![0.0f32; PAUSE_HOPS * HOP + 800]);
        assert!(seg.push(&stream).is_empty());
        // and the next one is found again
        let mut next = tone(8_000, 0.3);
        next.extend(vec![0.0f32; PAUSE_HOPS * HOP + 800]);
        assert_eq!(seg.push(&next).len(), 1);
    }
}
