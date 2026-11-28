//! Mel-frequency cepstral coefficients on 16 kHz mono audio, in pure Rust.
//!
//! The chain is the standard one: pre-emphasis, overlapping Hamming windows, a radix-2 FFT,
//! a mel filterbank, the log energies and a DCT-II. The result is [`COEFFS`] coefficients per
//! frame; the detectors drop the first (the loudness) and compare the rest by cosine distance,
//! which makes them level-insensitive.

use std::f32::consts::PI;

/// Sample rate the DSP expects.
pub const SAMPLE_RATE: u32 = 16_000;
/// Frame length (samples): 25 ms.
pub const FRAME: usize = 400;
/// Hop between frames (samples): 10 ms.
pub const HOP: usize = 160;
/// FFT size (a power of two above [`FRAME`]).
pub const FFT: usize = 512;
/// Number of mel filters.
pub const FILTERS: usize = 26;
/// Number of coefficients kept.
pub const COEFFS: usize = 13;

const PRE_EMPHASIS: f32 = 0.97;

/// The mel scale of a frequency in Hertz.
fn mel(hz: f32) -> f32 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

/// The frequency in Hertz of a mel value.
fn hz(mel: f32) -> f32 {
    700.0 * (10f32.powf(mel / 2595.0) - 1.0)
}

/// In-place iterative radix-2 FFT over `re` (length a power of two, `im` all zero).
fn radix2(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        let mut i = 0;
        while i < n {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (ur, ui) = (re[i + k], im[i + k]);
                let (vr, vi) = (
                    re[i + k + len / 2] * cr - im[i + k + len / 2] * ci,
                    re[i + k + len / 2] * ci + im[i + k + len / 2] * cr,
                );
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + len / 2] = ur - vr;
                im[i + k + len / 2] = ui - vi;
                let (nr, ni) = (cr * wr - ci * wi, cr * wi + ci * wr);
                cr = nr;
                ci = ni;
            }
            i += len;
        }
        len <<= 1;
    }
}

/// The mel filterbank: row `f` holds the weight per FFT bin.
fn filterbank() -> Vec<Vec<f32>> {
    let nyquist = SAMPLE_RATE as f32 / 2.0;
    let points = FILTERS + 2;
    let bins = FFT / 2 + 1;
    let mut centres = Vec::with_capacity(points);
    for i in 0..points {
        let m = mel(0.0) + (mel(nyquist) - mel(0.0)) * (i as f32 / (points - 1) as f32);
        centres.push((bins as f32 * hz(m) / (SAMPLE_RATE as f32)).floor() as usize);
    }
    let mut bank = vec![vec![0.0f32; bins]; FILTERS];
    for f in 0..FILTERS {
        let (lo, mid, hi) = (centres[f], centres[f + 1], centres[f + 2]);
        for (k, w) in bank[f].iter_mut().enumerate().take(mid).skip(lo) {
            *w = (k - lo) as f32 / (mid - lo) as f32;
        }
        for (k, w) in bank[f].iter_mut().enumerate().take(hi).skip(mid) {
            *w = (hi - k) as f32 / (hi - mid) as f32;
        }
    }
    bank
}

/// A prepared MFCC extractor (its filterbank is built once).
pub struct Mfcc {
    bank: Vec<Vec<f32>>,
}

impl Default for Mfcc {
    fn default() -> Self {
        Self::new()
    }
}

impl Mfcc {
    /// Builds the filterbank.
    pub fn new() -> Self {
        Self { bank: filterbank() }
    }

    /// The [`COEFFS`] coefficients of one [`FRAME`]-sample frame.
    pub fn coeffs(&self, frame: &[f32]) -> Vec<f32> {
        // Pre-emphasis and Hamming window, padded to the FFT size.
        let mut re = vec![0.0f32; FFT];
        for i in 0..FRAME {
            let x = frame[i];
            let earlier = if i == 0 { 0.0 } else { frame[i - 1] };
            let window = 0.54 - 0.46 * (2.0 * PI * i as f32 / (FRAME - 1) as f32).cos();
            re[i] = (x - PRE_EMPHASIS * earlier) * window;
        }
        let mut im = vec![0.0f32; FFT];
        radix2(&mut re, &mut im);
        // Mel energies of the power spectrum.
        let mut energies = [0.0f32; FILTERS];
        for (f, row) in self.bank.iter().enumerate() {
            let mut e = 0.0f32;
            for k in 0..row.len() {
                e += row[k] * (re[k] * re[k] + im[k] * im[k]);
            }
            energies[f] = (e + 1e-10).ln();
        }
        // DCT-II to the cepstrum.
        (0..COEFFS)
            .map(|k| {
                let mut c = 0.0f32;
                for (n, e) in energies.iter().enumerate() {
                    c += e * (PI * k as f32 * (2 * n + 1) as f32 / (2 * FILTERS) as f32).cos();
                }
                c
            })
            .collect()
    }

    /// All frames of a recording; empty when the recording is shorter than one frame.
    pub fn frames(&self, samples: &[f32]) -> Vec<Vec<f32>> {
        if samples.len() < FRAME {
            return Vec::new();
        }
        (0..=samples.len() - FRAME)
            .step_by(HOP)
            .map(|start| self.coeffs(&samples[start..start + FRAME]))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| (2.0 * PI * hz * i as f32 / SAMPLE_RATE as f32).sin() * 0.2)
            .collect()
    }

    #[test]
    fn coefficients_are_finite_and_deterministic() {
        let mfcc = Mfcc::new();
        let a = mfcc.coeffs(&tone(440.0, FRAME));
        let b = mfcc.coeffs(&tone(440.0, FRAME));
        assert_eq!(a, b);
        assert!(a.iter().all(|c| c.is_finite()));
        assert_eq!(a.len(), COEFFS);
    }

    #[test]
    fn different_tones_give_different_embeddings() {
        let mfcc = Mfcc::new();
        let low = mfcc.frames(&tone(200.0, 8_000));
        let high = mfcc.frames(&tone(2_000.0, 8_000));
        let mean = |frames: &[Vec<f32>]| {
            let mut sum = [0.0f32; COEFFS];
            for f in frames {
                for (i, v) in f.iter().enumerate() {
                    sum[i] += v;
                }
            }
            sum.iter()
                .map(|s| s / frames.len() as f32)
                .collect::<Vec<_>>()
        };
        assert!(low.len() > 10 && high.len() > 10);
        // Distinct spectral envelopes: notable distance between the means.
        let a = mean(&low);
        let b = mean(&high);
        let diff = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).sum::<f32>();
        assert!(diff > 1.0, "{diff}");
    }

    #[test]
    fn a_short_recording_has_no_frames() {
        assert!(Mfcc::new().frames(&tone(440.0, FRAME - 1)).is_empty());
    }
}
