//! Sample rate conversion: a streaming windowed-sinc resampler for one channel.
//!
//! The kernel's card runs at 48 kHz; programs play and record at their own rate (a call at
//! 16 kHz, a web page at 44.1 kHz). The filter is a table of the sinc kernel (Hann window,
//! cut-off at the lower of the two Nyquist rates) with linear interpolation between table
//! points, so a conversion costs a few dozen multiplications per output sample and no
//! trigonometry. The gain at 0 Hz is exactly 1.

use std::collections::VecDeque;

/// Zero crossings on each side of the kernel (in units of the lower rate).
const HALF_WIDTH: f64 = 12.0;
/// Table points per input sample.
const RESOLUTION: usize = 128;

/// A streaming resampler for one channel.
pub struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// Kernel half width in input samples.
    width: f64,
    table: Vec<f32>,
    buf: VecDeque<f32>,
    /// Position of the next output sample in `buf`.
    pos: f64,
    pass: bool,
}

impl Resampler {
    /// A resampler from `from` to `to` Hertz.
    pub fn new(from: u32, to: u32) -> Self {
        let pass = from == to;
        let cutoff = (f64::from(to) / f64::from(from)).min(1.0) * 0.97;
        let width = if cutoff > 0.0 {
            HALF_WIDTH / cutoff.max(0.01)
        } else {
            HALF_WIDTH
        };
        let points = (width * RESOLUTION as f64).ceil() as usize + 2;
        let mut table = Vec::with_capacity(points);
        for i in 0..points {
            let x = i as f64 / RESOLUTION as f64;
            let v = if x >= width {
                0.0
            } else {
                let arg = std::f64::consts::PI * cutoff * x;
                let sinc = if x == 0.0 { 1.0 } else { arg.sin() / arg };
                let hann = 0.5 + 0.5 * (std::f64::consts::PI * x / width).cos();
                cutoff * sinc * hann
            };
            table.push(v as f32);
        }
        let lead = width.ceil() as usize;
        Self {
            step: f64::from(from) / f64::from(to),
            width,
            table,
            buf: std::iter::repeat_n(0.0, lead).collect(),
            pos: lead as f64,
            pass,
        }
    }

    fn weight(&self, x: f64) -> f32 {
        let a = x.abs() * RESOLUTION as f64;
        let i = a as usize;
        let f = (a - i as f64) as f32;
        match (self.table.get(i), self.table.get(i + 1)) {
            (Some(&p), Some(&q)) => p + (q - p) * f,
            _ => 0.0,
        }
    }

    /// Converts `input` and appends the output samples to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.pass {
            out.extend_from_slice(input);
            return;
        }
        self.buf.extend(input.iter().copied());
        let reach = self.width.ceil() as usize;
        while self.pos + reach as f64 + 1.0 < self.buf.len() as f64 {
            let centre = self.pos.floor() as isize;
            let from = (centre - reach as isize + 1).max(0) as usize;
            let to = (centre + reach as isize).min(self.buf.len() as isize - 1) as usize;
            let (mut sum, mut weights) = (0.0f32, 0.0f32);
            for k in from..=to {
                let w = self.weight(k as f64 - self.pos);
                sum += self.buf[k] * w;
                weights += w;
            }
            out.push(if weights.abs() > 1e-6 { sum / weights } else { 0.0 });
            self.pos += self.step;
        }
        let drop = (self.pos.floor() as usize).saturating_sub(reach);
        self.buf.drain(..drop.min(self.buf.len()));
        self.pos -= drop as f64;
    }
}

/// `i16` samples as `f32` in `-1.0..1.0`.
pub fn to_f32(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|&s| f32::from(s) / 32768.0).collect()
}

/// `f32` samples back to `i16` (rounded, saturating).
pub fn to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| (s * 32768.0).round().clamp(-32768.0, 32767.0) as i16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin())
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn run(from: u32, to: u32, input: &[f32]) -> Vec<f32> {
        let mut r = Resampler::new(from, to);
        let mut out = Vec::new();
        r.process(input, &mut out);
        out
    }

    #[test]
    fn a_tone_keeps_its_level_in_both_directions() {
        let down = run(48_000, 16_000, &sine(48_000, 1000.0, 48_000));
        assert!((down.len() as i64 - 16_000).abs() < 40, "{}", down.len());
        let want = 0.5 / 2f32.sqrt();
        assert!((rms(&down[400..]) - want).abs() < 0.01 * want + 0.002);
        let up = run(16_000, 48_000, &sine(16_000, 1000.0, 16_000));
        assert!((up.len() as i64 - 48_000).abs() < 120, "{}", up.len());
        assert!((rms(&up[1200..]) - want).abs() < 0.01 * want + 0.002);
        let odd = run(44_100, 48_000, &sine(44_100, 3000.0, 44_100));
        assert!((odd.len() as i64 - 48_000).abs() < 120);
        assert!((rms(&odd[1200..]) - want).abs() < 0.015 * want + 0.002);
    }

    #[test]
    fn what_lies_above_the_new_nyquist_rate_is_removed() {
        // 12 kHz does not exist at 16 kHz: it must not fold down to 4 kHz
        let down = run(48_000, 16_000, &sine(48_000, 12_000.0, 48_000));
        let level = 20.0 * (rms(&down[400..]) / (0.5 / 2f32.sqrt())).log10();
        assert!(level < -40.0, "alias at {level} dB");
    }

    #[test]
    fn chunks_give_the_same_samples_as_one_run_and_equal_rates_pass() {
        let input = sine(44_100, 500.0, 20_000);
        let whole = run(44_100, 48_000, &input);
        let mut r = Resampler::new(44_100, 48_000);
        let mut parts = Vec::new();
        for c in input.chunks(333) {
            r.process(c, &mut parts);
        }
        assert_eq!(whole.len(), parts.len());
        assert!(whole.iter().zip(&parts).all(|(a, b)| (a - b).abs() < 1e-5));
        let same = run(48_000, 48_000, &input);
        assert_eq!(same, input);
    }

    #[test]
    fn integer_conversion_saturates() {
        assert_eq!(to_i16(&[2.0, -2.0, 0.0]), vec![32767, -32768, 0]);
        let back = to_i16(&to_f32(&[-32768, -1, 0, 1, 12345, 32767]));
        assert_eq!(back, vec![-32768, -1, 0, 1, 12345, 32767]);
    }
}
