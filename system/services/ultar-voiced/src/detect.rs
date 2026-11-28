//! The wake word, the secret word and the voice print of the person: small pure-Rust
//! templates built from the enrolment takes.
//!
//! A word is the cepstral sequence of each take; an utterance is the word when its
//! dynamic-time-warping distance to the closest take is below the threshold the enrolment
//! worked out from the takes themselves. The voice print is the long-term voice vector of
//! the enrolment sentences; a request is the person's when it lies near it.
//!
//! This is a template matcher, not a neural model: it knows the person's own pronunciation
//! of the word (and refuses most other words and most other voices), it is cheap enough to
//! run on a phone, and it needs no model file. `docs/integration/ultar-voiced.md` says what
//! it can and cannot do.

use crate::features::{dtw, sequence, vector_distance, voice_vector};
use crate::mfcc::Mfcc;
use serde::{Deserialize, Serialize};

/// The word threshold is this much above the widest distance between the person's takes.
const WORD_MARGIN: f32 = 1.35;
/// But never below this (a person who says it exactly the same three times).
const WORD_FLOOR: f32 = 1.0;
/// The voice threshold is this much above the widest distance between the takes.
const VOICE_MARGIN: f32 = 2.0;
/// But never below this.
const VOICE_FLOOR: f32 = 8.0;
/// A take may be this far (DTW distance) from the first one before it is refused as
/// "another word". Same word at another speed: about 8 on synthetic words; another word: 35+.
const TAKE_LIMIT: f32 = 15.0;

/// The enrolled wake word (or secret word): the takes and the threshold.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct WakeWord {
    takes: Vec<Vec<Vec<f32>>>,
    threshold: f32,
}

/// Why a take or a set of takes was not accepted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TakeError {
    /// Too short or too quiet to hold a word.
    TooShort,
    /// It does not sound like the earlier takes.
    Differs,
}

impl WakeWord {
    /// Whether `take` can join `earlier` takes (same word, long enough).
    pub fn accepts_take(mfcc: &Mfcc, earlier: &[Vec<f32>], take: &[f32]) -> Result<(), TakeError> {
        let Some(new) = sequence(mfcc, take) else {
            return Err(TakeError::TooShort);
        };
        let Some(first) = earlier.first().and_then(|e| sequence(mfcc, e)) else {
            return Ok(());
        };
        // A very different word is far beyond what two takes of one word differ by.
        let d = dtw(&first, &new);
        if d > TAKE_LIMIT {
            return Err(TakeError::Differs);
        }
        Ok(())
    }

    /// Builds the word from its takes (at least two).
    pub fn from_takes(mfcc: &Mfcc, takes: &[Vec<f32>]) -> Result<Self, TakeError> {
        let seqs: Vec<Vec<Vec<f32>>> = takes.iter().filter_map(|t| sequence(mfcc, t)).collect();
        if seqs.len() != takes.len() || seqs.len() < 2 {
            return Err(TakeError::TooShort);
        }
        let mut widest = 0.0f32;
        for (i, a) in seqs.iter().enumerate() {
            for b in &seqs[i + 1..] {
                let d = dtw(a, b);
                if d == f32::MAX {
                    return Err(TakeError::Differs);
                }
                widest = widest.max(d);
            }
        }
        Ok(Self {
            takes: seqs,
            threshold: (widest * WORD_MARGIN).max(WORD_FLOOR),
        })
    }

    /// The distance of an utterance to the closest take (`f32::MAX`: too short or too far).
    pub fn distance(&self, mfcc: &Mfcc, utterance: &[f32]) -> f32 {
        let Some(seq) = sequence(mfcc, utterance) else {
            return f32::MAX;
        };
        self.takes
            .iter()
            .map(|t| dtw(t, &seq))
            .fold(f32::MAX, f32::min)
    }

    /// Whether the utterance is the enrolled word.
    pub fn matches(&self, mfcc: &Mfcc, utterance: &[f32]) -> bool {
        self.distance(mfcc, utterance) <= self.threshold
    }

    /// The threshold worked out at enrolment.
    pub fn threshold(&self) -> f32 {
        self.threshold
    }
}

/// The enrolled voice print of the person, used to refuse other speakers.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct VoicePrint {
    mean: Vec<f32>,
    threshold: f32,
}

impl VoicePrint {
    /// Builds the print from the enrolment sentences (at least two).
    pub fn from_takes(mfcc: &Mfcc, takes: &[Vec<f32>]) -> Result<Self, TakeError> {
        let vectors: Vec<Vec<f32>> = takes.iter().filter_map(|t| voice_vector(mfcc, t)).collect();
        if vectors.len() != takes.len() || vectors.len() < 2 {
            return Err(TakeError::TooShort);
        }
        let n = vectors.len() as f32;
        let mut mean = vec![0.0f32; vectors[0].len()];
        for v in &vectors {
            for (m, x) in mean.iter_mut().zip(v) {
                *m += x / n;
            }
        }
        let widest = vectors
            .iter()
            .flat_map(|a| vectors.iter().map(move |b| vector_distance(a, b)))
            .fold(0.0f32, f32::max);
        Ok(Self {
            mean,
            threshold: (widest * VOICE_MARGIN).max(VOICE_FLOOR),
        })
    }

    /// Distance of an utterance to the print (`f32::MAX`: too little speech).
    pub fn distance(&self, mfcc: &Mfcc, utterance: &[f32]) -> f32 {
        voice_vector(mfcc, utterance).map_or(f32::MAX, |v| vector_distance(&self.mean, &v))
    }

    /// Whether the utterance is the person's voice. Too little speech to tell is "no".
    pub fn accepts(&self, mfcc: &Mfcc, utterance: &[f32]) -> bool {
        self.distance(mfcc, utterance) <= self.threshold
    }

    /// The threshold worked out at enrolment.
    pub fn threshold(&self) -> f32 {
        self.threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// A synthetic word: bursts around `base` Hz, `speed` stretches it in time.
    fn word(base: f32, speed: f32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| {
                let t = i as f32 / 16_000.0 / speed;
                let burst = 0.55 + 0.45 * (2.0 * PI * 3.0 * t).sin();
                let glide = base * (1.0 + 0.3 * (2.0 * PI * 1.5 * t).sin());
                (2.0 * PI * glide * t).sin() * 0.25 * burst
            })
            .collect()
    }

    #[test]
    fn the_word_matches_new_takes_and_not_another_word() {
        let mfcc = Mfcc::new();
        let takes: Vec<Vec<f32>> = [1.0f32, 1.08, 0.94]
            .iter()
            .map(|s| word(400.0, *s, (12_000.0 * s) as usize))
            .collect();
        let wake = WakeWord::from_takes(&mfcc, &takes).unwrap();
        assert!(wake.matches(&mfcc, &word(400.0, 1.03, 12_400)));
        assert!(!wake.matches(&mfcc, &word(1_900.0, 1.0, 12_000)));
        assert!(!wake.matches(&mfcc, &vec![0.0f32; 12_000]));
        assert!(wake.threshold() >= 1.0);
    }

    #[test]
    fn a_take_of_another_word_or_a_short_one_is_refused() {
        let mfcc = Mfcc::new();
        let first = vec![word(400.0, 1.0, 12_000)];
        assert_eq!(
            WakeWord::accepts_take(&mfcc, &first, &word(400.0, 1.05, 12_600)),
            Ok(())
        );
        assert_eq!(
            WakeWord::accepts_take(&mfcc, &first, &word(1_900.0, 1.0, 12_000)),
            Err(TakeError::Differs)
        );
        assert_eq!(
            WakeWord::accepts_take(&mfcc, &first, &word(400.0, 1.0, 900)),
            Err(TakeError::TooShort)
        );
        assert_eq!(
            WakeWord::from_takes(&mfcc, &[word(400.0, 1.0, 12_000)]),
            Err(TakeError::TooShort)
        );
    }

    #[test]
    fn the_voice_print_accepts_the_speaker_and_refuses_another() {
        let mfcc = Mfcc::new();
        let takes: Vec<Vec<f32>> = [1.0f32, 1.1, 0.9]
            .iter()
            .map(|s| word(500.0, *s, (24_000.0 * s) as usize))
            .collect();
        let print = VoicePrint::from_takes(&mfcc, &takes).unwrap();
        assert!(print.accepts(&mfcc, &word(500.0, 1.05, 30_000)));
        assert!(!print.accepts(&mfcc, &word(2_600.0, 1.0, 24_000)));
        assert!(!print.accepts(&mfcc, &word(500.0, 1.0, 2_000)), "too short");
    }

    #[test]
    fn templates_survive_the_state_file() {
        let mfcc = Mfcc::new();
        let takes: Vec<Vec<f32>> = [1.0f32, 1.08]
            .iter()
            .map(|s| word(400.0, *s, (12_000.0 * s) as usize))
            .collect();
        let wake = WakeWord::from_takes(&mfcc, &takes).unwrap();
        let bytes = ultar_bus::encode(&wake).unwrap();
        let back: WakeWord = ultar_bus::decode(&bytes).unwrap();
        assert_eq!(back, wake);
    }
}
