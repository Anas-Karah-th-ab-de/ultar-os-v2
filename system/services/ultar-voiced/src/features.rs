//! What the detectors compare: the cepstral sequence of a spoken word (for the wake word,
//! matched by dynamic time warping) and the long-term voice vector of a longer utterance (for
//! the speaker check).

use crate::mfcc::{Mfcc, COEFFS, FRAME, HOP};
use crate::vad::rms;

/// Coefficients compared (all but the first, which is the loudness).
pub const DIM: usize = COEFFS - 1;
/// Fewest frames (after the step) of a word: 200 ms.
pub const MIN_WORD_FRAMES: usize = 10;
/// Fewest speech frames (10 ms) for a voice vector: 0.4 s of speech.
pub const MIN_VOICE_FRAMES: usize = 40;
/// Every second frame is compared: 20 ms steps are enough for words and twice as fast.
const STEP: usize = 2;
/// A frame is speech when its level is this share of the loudest frame.
const RELATIVE_GATE: f32 = 0.12;
/// And at least this loud (root-mean-square).
const ABS_GATE: f32 = 0.008;

fn frame_levels(samples: &[f32]) -> Vec<f32> {
    if samples.len() < FRAME {
        return Vec::new();
    }
    (0..=samples.len() - FRAME)
        .step_by(HOP)
        .map(|start| rms(&samples[start..start + FRAME]))
        .collect()
}

fn gate(levels: &[f32]) -> f32 {
    let peak = levels.iter().copied().fold(0.0f32, f32::max);
    (peak * RELATIVE_GATE).max(ABS_GATE)
}

/// The cepstral sequence of one spoken word: trimmed to its speech, the long-term mean
/// removed (so the microphone and the room drop out), every second frame. `None` when it is
/// shorter than [`MIN_WORD_FRAMES`].
pub fn sequence(mfcc: &Mfcc, samples: &[f32]) -> Option<Vec<Vec<f32>>> {
    let levels = frame_levels(samples);
    let limit = gate(&levels);
    let first = levels.iter().position(|l| *l >= limit)?;
    let last = levels.iter().rposition(|l| *l >= limit)?;
    let frames = mfcc.frames(samples);
    let kept: Vec<&Vec<f32>> = frames
        .get(first..=last.min(frames.len().saturating_sub(1)))?
        .iter()
        .collect();
    if kept.is_empty() {
        return None;
    }
    let mut mean = vec![0.0f32; DIM];
    for f in &kept {
        for (i, v) in f.iter().skip(1).enumerate() {
            mean[i] += v;
        }
    }
    for m in &mut mean {
        *m /= kept.len() as f32;
    }
    let seq: Vec<Vec<f32>> = kept
        .iter()
        .step_by(STEP)
        .map(|f| f.iter().skip(1).zip(&mean).map(|(v, m)| v - m).collect())
        .collect();
    (seq.len() >= MIN_WORD_FRAMES).then_some(seq)
}

fn distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f32>()
        .sqrt()
}

/// Dynamic-time-warping distance of two sequences, per step of the best path. Sequences whose
/// lengths differ by more than a factor of two are never the same word (`f32::MAX`).
pub fn dtw(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    let (n, m) = (a.len(), b.len());
    if n == 0 || m == 0 || n > 2 * m || m > 2 * n {
        return f32::MAX;
    }
    let band = (n.max(m) * 2 / 5).max(8);
    let inf = f32::MAX / 4.0;
    let mut prev = vec![inf; m + 1];
    let mut cur = vec![inf; m + 1];
    prev[0] = 0.0;
    for i in 1..=n {
        cur.iter_mut().for_each(|c| *c = inf);
        let centre = i * m / n;
        let lo = centre.saturating_sub(band).max(1);
        let hi = (centre + band).min(m);
        for j in lo..=hi {
            let cost = distance(&a[i - 1], &b[j - 1]);
            let best = prev[j].min(cur[j - 1]).min(prev[j - 1]);
            cur[j] = cost + best;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let total = prev[m];
    if total >= inf {
        f32::MAX
    } else {
        total / (n + m) as f32
    }
}

/// The long-term voice vector of an utterance: mean and standard deviation of every
/// coefficient over the speech frames (`2 * DIM` numbers). `None` when there is too little
/// speech.
pub fn voice_vector(mfcc: &Mfcc, samples: &[f32]) -> Option<Vec<f32>> {
    let levels = frame_levels(samples);
    let limit = gate(&levels);
    let frames = mfcc.frames(samples);
    let speech: Vec<&Vec<f32>> = frames
        .iter()
        .zip(&levels)
        .filter(|(_, l)| **l >= limit)
        .map(|(f, _)| f)
        .collect();
    if speech.len() < MIN_VOICE_FRAMES {
        return None;
    }
    let n = speech.len() as f32;
    let mut out = vec![0.0f32; 2 * DIM];
    for f in &speech {
        for (i, v) in f.iter().skip(1).enumerate() {
            out[i] += v;
        }
    }
    for m in out.iter_mut().take(DIM) {
        *m /= n;
    }
    for f in &speech {
        for (i, v) in f.iter().skip(1).enumerate() {
            let d = v - out[i];
            out[DIM + i] += d * d;
        }
    }
    for s in out.iter_mut().skip(DIM) {
        *s = (*s / n).sqrt();
    }
    Some(out)
}

/// Euclidean distance of two voice vectors.
pub fn vector_distance(a: &[f32], b: &[f32]) -> f32 {
    distance(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// A synthetic word: bursts around `base` Hz, `speed` stretches it in time.
    pub(crate) fn word(base: f32, speed: f32, len: usize) -> Vec<f32> {
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
    fn the_same_word_is_close_and_another_far() {
        let mfcc = Mfcc::new();
        let a = sequence(&mfcc, &word(400.0, 1.0, 12_000)).unwrap();
        let slower = sequence(&mfcc, &word(400.0, 1.15, 13_800)).unwrap();
        let other = sequence(&mfcc, &word(1_800.0, 1.0, 12_000)).unwrap();
        let near = dtw(&a, &slower);
        let far = dtw(&a, &other);
        assert!(near < far, "{near} {far}");
        assert!(dtw(&a, &a) < 1e-3);
    }

    #[test]
    fn lengths_far_apart_never_match_and_short_words_have_no_sequence() {
        let mfcc = Mfcc::new();
        let a = sequence(&mfcc, &word(400.0, 1.0, 12_000)).unwrap();
        let b: Vec<Vec<f32>> = a.iter().take(a.len() / 3).cloned().collect();
        assert_eq!(dtw(&a, &b), f32::MAX);
        assert_eq!(dtw(&[], &a), f32::MAX);
        assert!(sequence(&mfcc, &word(400.0, 1.0, 1_000)).is_none());
        assert!(sequence(&mfcc, &vec![0.0f32; 16_000]).is_none());
    }

    #[test]
    fn a_voice_vector_needs_enough_speech() {
        let mfcc = Mfcc::new();
        assert!(voice_vector(&mfcc, &word(400.0, 1.0, 3_000)).is_none());
        let v = voice_vector(&mfcc, &word(400.0, 1.0, 16_000)).unwrap();
        assert_eq!(v.len(), 2 * DIM);
        assert!(v.iter().all(|x| x.is_finite()));
    }
}
