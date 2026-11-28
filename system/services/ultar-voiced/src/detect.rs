//! Wake word, secret word and voice print: small pure-Rust templates compared by cosine
//! distance over the MFCC embedding (all coefficients but the loudness, so the matching
//! does not depend on how loud the person spoke).

use crate::mfcc::{Mfcc, COEFFS};
use serde::{Deserialize, Serialize};

/// Coefficients used for comparison (all but the first, the loudness).
const DIM: usize = COEFFS - 1;
/// Cosine distance that counts as the wake or secret word.
const WAKE_THRESHOLD: f32 = 0.70;
/// Cosine distance that counts as the person's voice.
const VOICE_THRESHOLD: f32 = 0.60;

/// The mean MFCC embedding of a recording (coefficients 1.., so level-insensitive).
pub fn embedding(mfcc: &Mfcc, samples: &[f32]) -> Option<Vec<f32>> {
    let frames = mfcc.frames(samples);
    if frames.is_empty() {
        return None;
    }
    let mut sum = vec![0.0f32; DIM];
    for f in &frames {
        for (i, v) in f.iter().enumerate().skip(1) {
            sum[i - 1] += v;
        }
    }
    let n = frames.len() as f32;
    Some(sum.iter().map(|s| s / n).collect())
}

/// The cosine distance of two same-length vectors (`0.0` when one is zero).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (dot, na, nb) = a
        .iter()
        .zip(b)
        .fold((0.0f32, 0.0f32, 0.0f32), |(dot, na, nb), (&x, &y)| {
            (dot + x * y, na + x * x, nb + y * y)
        });
    if na <= 0.0 || nb <= 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

fn embedding_of_clips(mfcc: &Mfcc, clips: &[Vec<f32>]) -> Option<Vec<f32>> {
    let mut all = Vec::new();
    for clip in clips {
        all.extend_from_slice(clip);
    }
    embedding(mfcc, &all)
}

/// The enrolled wake word (or secret word): its template and its threshold.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct WakeWord {
    template: Vec<f32>,
    threshold: f32,
}

impl WakeWord {
    /// Builds the template as the mean embedding of the enrolment clips (`None`: too short).
    pub fn from_clips(mfcc: &Mfcc, clips: &[Vec<f32>]) -> Option<Self> {
        let template = embedding_of_clips(mfcc, clips)?;
        Some(Self {
            template,
            threshold: WAKE_THRESHOLD,
        })
    }

    /// Empty template for the default (used in tests to build a detector by hand).
    pub fn with_template(template: Vec<f32>, threshold: f32) -> Self {
        Self {
            template,
            threshold,
        }
    }

    /// Cosine of the recording against the template (`0.0` when it is too short).
    pub fn score(&self, mfcc: &Mfcc, samples: &[f32]) -> f32 {
        embedding(mfcc, samples)
            .map(|e| cosine(&self.template, &e))
            .unwrap_or(0.0)
    }

    /// Whether the recording is the enrolled word.
    pub fn matches(&self, mfcc: &Mfcc, samples: &[f32]) -> bool {
        self.score(mfcc, samples) >= self.threshold
    }
}

/// The enrolled voice print of the person, used to refuse other speakers.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct VoicePrint {
    mean: Vec<f32>,
    threshold: f32,
}

impl VoicePrint {
    /// The print is the mean embedding over the enrolment sentence(s).
    pub fn from_clips(mfcc: &Mfcc, clips: &[Vec<f32>]) -> Option<Self> {
        let mean = embedding_of_clips(mfcc, clips)?;
        Some(Self {
            mean,
            threshold: VOICE_THRESHOLD,
        })
    }

    /// Cosine of the recording against the print.
    pub fn similarity(&self, mfcc: &Mfcc, samples: &[f32]) -> f32 {
        embedding(mfcc, samples)
            .map(|e| cosine(&self.mean, &e))
            .unwrap_or(0.0)
    }

    /// Whether the recording is the person's voice.
    pub fn accepts(&self, mfcc: &Mfcc, samples: &[f32]) -> bool {
        self.similarity(mfcc, samples) >= self.threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// A deterministic band-limited "word": tone bursts on a base frequency, slightly
    /// varied by `seed` to sound like three takes of the same word.
    fn word(base: f32, seed: f32, len: usize) -> Vec<f32> {
        let rate = crate::mfcc::SAMPLE_RATE as f32;
        (0..len)
            .map(|i| {
                let t = i as f32 / rate;
                let burst = 0.5 + 0.5 * (2.0 * PI * 3.0 * t).sin();
                let wobble = 1.0 + seed * 0.05 * (2.0 * PI * 7.0 * t).sin();
                (2.0 * PI * base * wobble * t).sin() * 0.25 * burst
            })
            .collect()
    }

    #[test]
    fn the_word_matches_its_own_takes_and_not_another_word() {
        let mfcc = Mfcc::new();
        let clips = (0..3)
            .map(|k| word(400.0, k as f32, 12_000))
            .collect::<Vec<_>>();
        let wake = WakeWord::from_clips(&mfcc, &clips).unwrap();
        // A new take of the same word matches; a different word does not.
        assert!(wake.matches(&mfcc, &word(400.0, 4.0, 12_000)));
        assert!(!wake.matches(&mfcc, &word(2_000.0, 0.0, 12_000)));
        assert!(wake.matches(&mfcc, &word(400.0, 0.0, 12_000)));
    }

    #[test]
    fn the_voice_print_accepts_the_speaker_and_refuses_another() {
        let mfcc = Mfcc::new();
        let clips = (0..3)
            .map(|k| word(500.0, k as f32, 16_000))
            .collect::<Vec<_>>();
        let print = VoicePrint::from_clips(&mfcc, &clips).unwrap();
        assert!(print.accepts(&mfcc, &word(500.0, 5.0, 16_000)));
        assert!(!print.accepts(&mfcc, &word(2_400.0, 0.0, 16_000)));
    }

    #[test]
    fn short_recordings_give_no_embedding() {
        let mfcc = Mfcc::new();
        assert!(embedding(&mfcc, &vec![0.0f32; 100]).is_none());
        assert!(WakeWord::from_clips(&mfcc, &[vec![0.0f32; 100]]).is_none());
    }
}