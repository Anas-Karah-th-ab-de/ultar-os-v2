//! Echo cancellation for calls on the device's own speaker.
//!
//! A partitioned-block frequency-domain adaptive filter (PBFDAF, overlap-save): the far
//! end (what the speaker plays, taken from the mixer's output) is filtered with an
//! estimate of the room's impulse response and subtracted from the microphone. 16 kHz
//! mono, blocks of 8 ms, 16 partitions = 128 ms of echo path. Each frequency bin is
//! normalised by the power of the far end in it, so speech (whose spectrum is far from
//! flat) converges about as fast as noise.
//!
//! What it does **not** cover (documented in `docs/integration/ultar-audiod.md`): it is
//! linear (a speaker driven into distortion leaves a residual echo), has no residual echo
//! suppressor, does not follow a clock drift between speaker and microphone (it tracks
//! a few milliseconds of it), and its double-talk detector is a simple level rule.

use std::collections::VecDeque;

/// Samples of one block.
pub const BLOCK: usize = 128;
/// FFT size (two blocks).
const N: usize = 2 * BLOCK;
/// Partitions: 16 x 8 ms = 128 ms of echo path.
const PARTS: usize = 16;
/// Step size of the update.
const MU: f32 = 0.6;
/// Blocks the update stays frozen after double talk.
const HANGOVER: u32 = 4;
/// Samples the far end may run ahead of the microphone before the oldest are dropped (16 ms).
const MAX_LEAD: usize = 2 * BLOCK;
/// Longest time (in blocks, about 3 s) the filter is held before the room counts as changed.
const HOLD_MAX: u32 = 375;
/// Far-end level below which nothing is learned (the far end is silent).
const FAR_FLOOR: f32 = 0.001;

/// The FFT of one size (radix-2, in place).
struct Fft {
    cos: Vec<f32>,
    sin: Vec<f32>,
    rev: Vec<usize>,
}

impl Fft {
    fn new() -> Self {
        let bits = N.trailing_zeros();
        let rev = (0..N)
            .map(|i| i.reverse_bits() >> (usize::BITS - bits))
            .collect();
        let (cos, sin) = (0..N / 2)
            .map(|k| {
                let a = -2.0 * std::f64::consts::PI * k as f64 / N as f64;
                (a.cos() as f32, a.sin() as f32)
            })
            .unzip();
        Self { cos, sin, rev }
    }

    /// Forward transform; with `inverse` the inverse including the factor 1/N.
    fn run(&self, re: &mut [f32], im: &mut [f32], inverse: bool) {
        for i in 0..N {
            let j = self.rev[i];
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= N {
            let half = len / 2;
            let stride = N / len;
            for start in (0..N).step_by(len) {
                for k in 0..half {
                    let (wr, mut wi) = (self.cos[k * stride], self.sin[k * stride]);
                    if inverse {
                        wi = -wi;
                    }
                    let (a, b) = (start + k, start + k + half);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len *= 2;
        }
        if inverse {
            let s = 1.0 / N as f32;
            re.iter_mut().for_each(|v| *v *= s);
            im.iter_mut().for_each(|v| *v *= s);
        }
    }
}

/// The echo canceller of one call.
pub struct Aec {
    fft: Fft,
    far_q: VecDeque<f32>,
    near_q: VecDeque<f32>,
    /// Far end of the last two blocks.
    xbuf: [f32; N],
    /// Weights, partition after partition.
    w_re: Vec<f32>,
    w_im: Vec<f32>,
    /// Spectra of the far end, newest at `head`.
    x_re: Vec<f32>,
    x_im: Vec<f32>,
    head: usize,
    power: Vec<f32>,
    far_peaks: [f32; 8],
    peak_at: usize,
    hang: u32,
    blocks: u64,
    /// Slow averages of the microphone power and of what is left after the filter.
    pd_s: f32,
    pe_s: f32,
    /// Times the filter was thrown away, blocks it was held, blocks it learned.
    resets: u64,
    frozen: u64,
    learned: u64,
    /// Blocks in a row the filter was held, and blocks the averages stay frozen.
    held: u32,
    jump_hang: u32,
}

impl Default for Aec {
    fn default() -> Self {
        Self::new()
    }
}

impl Aec {
    /// A canceller that knows nothing yet about the room.
    pub fn new() -> Self {
        Self {
            fft: Fft::new(),
            far_q: VecDeque::new(),
            near_q: VecDeque::new(),
            xbuf: [0.0; N],
            w_re: vec![0.0; PARTS * N],
            w_im: vec![0.0; PARTS * N],
            x_re: vec![0.0; PARTS * N],
            x_im: vec![0.0; PARTS * N],
            head: 0,
            power: vec![1e-3; N],
            far_peaks: [0.0; 8],
            peak_at: 0,
            hang: 0,
            blocks: 0,
            pd_s: 0.0,
            pe_s: 0.0,
            resets: 0,
            frozen: 0,
            learned: 0,
            held: 0,
            jump_hang: 0,
        }
    }

    /// Blocks processed so far.
    pub fn blocks(&self) -> u64 {
        self.blocks
    }

    /// How often the filter was thrown away, held (double talk) and updated.
    pub fn counts(&self) -> (u64, u64, u64) {
        (self.resets, self.frozen, self.learned)
    }

    /// Feeds `far` (what was played) and `near` (what the microphone heard, at the same
    /// time) and appends the microphone without the echo to `out`. Output comes in blocks
    /// of [`BLOCK`]; what does not fill a block waits for the next call.
    pub fn process(&mut self, far: &[f32], near: &[f32], out: &mut Vec<f32>) {
        self.far_q.extend(far);
        self.near_q.extend(near);
        // The far end of a time the microphone never delivered (the card was not recording yet)
        // has no partner: left in the queue it would shift every later pair by that time and
        // push the echo out of the filter's window.
        let lead = self.far_q.len().saturating_sub(self.near_q.len());
        if lead > MAX_LEAD {
            self.far_q.drain(..lead - MAX_LEAD);
        }
        while self.far_q.len() >= BLOCK && self.near_q.len() >= BLOCK {
            let x: Vec<f32> = self.far_q.drain(..BLOCK).collect();
            let d: Vec<f32> = self.near_q.drain(..BLOCK).collect();
            self.block(&x, &d, out);
        }
        // never lag: a side that runs far ahead of the other is cut back
        for q in [&mut self.far_q, &mut self.near_q] {
            if q.len() > 8 * BLOCK {
                let cut = q.len() - BLOCK;
                q.drain(..cut);
            }
        }
    }

    fn block(&mut self, x: &[f32], d: &[f32], out: &mut Vec<f32>) {
        self.blocks += 1;
        self.xbuf.copy_within(BLOCK.., 0);
        self.xbuf[BLOCK..].copy_from_slice(x);
        self.head = (self.head + PARTS - 1) % PARTS;
        let base = self.head * N;
        self.x_re[base..base + N].copy_from_slice(&self.xbuf);
        self.x_im[base..base + N].fill(0.0);
        let (re, im) = (
            &mut self.x_re[base..base + N],
            &mut self.x_im[base..base + N],
        );
        self.fft.run(re, im, false);

        // echo estimate: sum over the partitions of weight x far spectrum
        let (mut yr, mut yi) = (vec![0.0f32; N], vec![0.0f32; N]);
        self.power.iter_mut().for_each(|p| *p *= 0.8);
        for p in 0..PARTS {
            let xs = ((self.head + p) % PARTS) * N;
            let ws = p * N;
            for k in 0..N {
                let (a, b) = (self.w_re[ws + k], self.w_im[ws + k]);
                let (c, e) = (self.x_re[xs + k], self.x_im[xs + k]);
                yr[k] += a * c - b * e;
                yi[k] += a * e + b * c;
                self.power[k] += 0.2 * (c * c + e * e);
            }
        }
        self.fft.run(&mut yr, &mut yi, true);
        let mut e = [0.0f32; BLOCK];
        let (mut pd, mut pe) = (0.0f32, 0.0f32);
        for i in 0..BLOCK {
            e[i] = d[i] - yr[BLOCK + i];
            pd += d[i] * d[i];
            pe += e[i] * e[i];
        }

        // the filter did harm (changed room, double talk gone wrong): start over
        if pe > 2.0 * pd + 1e-6 {
            self.w_re.fill(0.0);
            self.w_im.fill(0.0);
            self.resets += 1;
            out.extend_from_slice(d);
            return;
        }

        // double talk: the microphone is louder than the far end can explain
        let far_peak = x.iter().fold(0.0f32, |a, v| a.max(v.abs()));
        self.far_peaks[self.peak_at] = far_peak;
        self.peak_at = (self.peak_at + 1) % self.far_peaks.len();
        let window = self.far_peaks.iter().fold(0.0f32, |a, v| a.max(*v));
        let near_peak = d.iter().fold(0.0f32, |a, v| a.max(v.abs()));
        if near_peak > window {
            self.hang = HANGOVER;
        }
        // once the filter works, a sudden rise of what is left over means somebody talks
        // on this side (or the room changed; the average catches up within a quarter second)
        let converged = self.pd_s > 1e-8 && self.pe_s < 0.3 * self.pd_s;
        if converged && pe > 3.0 * self.pe_s + 1e-9 {
            self.hang = HANGOVER;
            self.jump_hang = HANGOVER;
        }
        if self.jump_hang > 0 {
            // the averages stay where they were, so a long talk keeps being recognised;
            // after HOLD_MAX blocks it is taken as a changed room and the filter learns again
            self.jump_hang -= 1;
            self.held += 1;
            if self.held > HOLD_MAX {
                self.held = 0;
                self.hang = 0;
                self.jump_hang = 0;
                self.pd_s = pd;
                self.pe_s = pe;
            }
        } else {
            self.held = 0;
            self.pd_s = 0.98 * self.pd_s + 0.02 * pd;
            self.pe_s = 0.98 * self.pe_s + 0.02 * pe;
        }
        let learn = window > FAR_FLOOR && self.hang == 0;
        self.hang = self.hang.saturating_sub(1);

        if window > FAR_FLOOR && !learn {
            self.frozen += 1;
        }
        if learn {
            self.learned += 1;
            let mut er = vec![0.0f32; N];
            let mut ei = vec![0.0f32; N];
            er[BLOCK..].copy_from_slice(&e);
            self.fft.run(&mut er, &mut ei, false);
            let floor = 1e-4 * N as f32;
            for p in 0..PARTS {
                let xs = ((self.head + p) % PARTS) * N;
                let ws = p * N;
                for k in 0..N {
                    let g = MU / (self.power[k] + floor);
                    let (c, s) = (self.x_re[xs + k], -self.x_im[xs + k]);
                    self.w_re[ws + k] += g * (c * er[k] - s * ei[k]);
                    self.w_im[ws + k] += g * (c * ei[k] + s * er[k]);
                }
            }
            // keep the weights causal and short: one partition per block
            let j = (self.blocks as usize) % PARTS;
            let ws = j * N;
            let mut wr = self.w_re[ws..ws + N].to_vec();
            let mut wi = self.w_im[ws..ws + N].to_vec();
            self.fft.run(&mut wr, &mut wi, true);
            wr[BLOCK..].fill(0.0);
            wi[BLOCK..].fill(0.0);
            self.fft.run(&mut wr, &mut wi, false);
            self.w_re[ws..ws + N].copy_from_slice(&wr);
            self.w_im[ws..ws + N].copy_from_slice(&wi);
        }
        out.extend_from_slice(&e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plain noise, repeatable.
    struct Noise(u64);

    impl Noise {
        fn next(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
        }
    }

    /// Speech-like far end: noise with a steep spectrum (first-order low pass).
    fn far_end(n: usize, seed: u64) -> Vec<f32> {
        let mut r = Noise(seed);
        let mut y = 0.0f32;
        (0..n)
            .map(|_| {
                y = 0.9 * y + 0.3 * r.next();
                y
            })
            .collect()
    }

    /// A room: 40 ms of delay, then 60 ms of a decaying tail; the echo is about 10 dB
    /// weaker than the far end (the usual loudspeaker and microphone of a desk).
    fn room(seed: u64) -> Vec<f32> {
        let mut r = Noise(seed);
        let mut h = vec![0.0f32; 640];
        for i in 0..960 {
            h.push(0.05 * r.next() * (-(i as f32) / 250.0).exp());
        }
        h
    }

    fn convolve(x: &[f32], h: &[f32]) -> Vec<f32> {
        (0..x.len())
            .map(|n| {
                h.iter()
                    .enumerate()
                    .take(n + 1)
                    .map(|(k, hk)| hk * x[n - k])
                    .sum()
            })
            .collect()
    }

    fn power(x: &[f32]) -> f32 {
        x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32
    }

    fn erle_db(echo: &[f32], residual: &[f32]) -> f32 {
        10.0 * (power(echo) / power(residual).max(1e-12)).log10()
    }

    #[test]
    fn the_fft_inverts_itself_and_finds_a_tone() {
        let fft = Fft::new();
        let mut re: Vec<f32> = (0..N)
            .map(|i| (2.0 * std::f32::consts::PI * 8.0 * i as f32 / N as f32).cos())
            .collect();
        let mut im = vec![0.0f32; N];
        let orig = re.clone();
        fft.run(&mut re, &mut im, false);
        let (peak, _) = re
            .iter()
            .enumerate()
            .take(N / 2)
            .fold((0, 0.0f32), |a, (i, v)| if *v > a.1 { (i, *v) } else { a });
        assert_eq!(peak, 8);
        assert!((re[8] - N as f32 / 2.0).abs() < 1e-2);
        fft.run(&mut re, &mut im, true);
        assert!(re.iter().zip(&orig).all(|(a, b)| (a - b).abs() < 1e-4));
    }

    #[test]
    fn echo_is_cancelled_by_more_than_30_db() {
        let n = 16_000 * 6;
        let far = far_end(n, 7);
        let echo = convolve(&far, &room(3));
        let mut noise = Noise(11);
        let near: Vec<f32> = echo.iter().map(|e| e + 0.0005 * noise.next()).collect();
        let mut aec = Aec::new();
        let mut out = Vec::new();
        for (f, d) in far.chunks(160).zip(near.chunks(160)) {
            aec.process(f, d, &mut out);
        }
        let tail = out.len() - 16_000;
        let erle = erle_db(&echo[tail..out.len()], &out[tail..]);
        println!("ERLE after 5 s, speech-like far end: {erle:.1} dB");
        assert!(erle > 30.0, "ERLE {erle} dB");
        // it learned in the first seconds already
        let early = erle_db(&echo[16_000 * 2..16_000 * 3], &out[16_000 * 2..16_000 * 3]);
        println!("ERLE in second 3: {early:.1} dB");
        assert!(early > 15.0, "ERLE in second 3: {early} dB");
    }

    #[test]
    fn the_person_on_this_side_is_heard_during_double_talk() {
        let n = 16_000 * 8;
        let far = far_end(n, 21);
        let echo = convolve(&far, &room(5));
        // converge first, then the near person speaks (a loud tone burst train)
        let voice: Vec<f32> = (0..n)
            .map(|i| {
                if i > 16_000 * 5 && (i / 2000) % 2 == 0 {
                    0.6 * (2.0 * std::f32::consts::PI * 300.0 * i as f32 / 16_000.0).sin()
                } else {
                    0.0
                }
            })
            .collect();
        let near: Vec<f32> = echo.iter().zip(&voice).map(|(e, v)| e + v).collect();
        let mut aec = Aec::new();
        let mut out = Vec::new();
        for (f, d) in far.chunks(160).zip(near.chunks(160)) {
            aec.process(f, d, &mut out);
        }
        // what comes out is the voice, with the echo far below it
        let from = 16_000 * 6;
        let to = out.len();
        let gaps: Vec<usize> = (from..to).filter(|i| voice[*i] == 0.0).collect();
        let g_echo: Vec<f32> = gaps.iter().map(|&i| echo[i]).collect();
        let g_out: Vec<f32> = gaps.iter().map(|&i| out[i]).collect();
        let gap_erle = erle_db(&g_echo, &g_out);
        println!("echo removed in the pauses of the near talker: {gap_erle:.1} dB");
        assert!(gap_erle > 15.0, "pauses only {gap_erle} dB");
        let err: Vec<f32> = (from..to).map(|i| out[i] - voice[i]).collect();
        let ser = 10.0 * (power(&voice[from..to]) / power(&err)).log10();
        println!("near speech over what is left, during double talk: {ser:.1} dB");
        assert!(ser > 12.0, "near speech only {ser} dB over the rest");
    }

    #[test]
    fn nothing_is_removed_when_the_far_end_is_silent_and_a_changed_room_is_learned_again() {
        let mut r = Noise(1);
        let near: Vec<f32> = (0..16_000).map(|_| 0.1 * r.next()).collect();
        let mut aec = Aec::new();
        let mut out = Vec::new();
        aec.process(&vec![0.0; near.len()], &near, &mut out);
        assert_eq!(out.len(), near.len() / BLOCK * BLOCK);
        assert!(out.iter().zip(&near).all(|(a, b)| (a - b).abs() < 1e-6));
        // the room changes after four seconds
        let far = far_end(16_000 * 10, 33);
        let mut echo = convolve(&far[..16_000 * 4], &room(8));
        echo.extend(&convolve(&far, &room(9))[16_000 * 4..]);
        let mut aec = Aec::new();
        let mut out = Vec::new();
        for (f, d) in far.chunks(160).zip(echo.chunks(160)) {
            aec.process(f, d, &mut out);
        }
        let end = out.len();
        let erle = erle_db(&echo[end - 16_000..end], &out[end - 16_000..]);
        println!("ERLE 6 s after a room change: {erle:.1} dB");
        assert!(erle > 25.0, "ERLE {erle} dB after a room change");
        assert!(aec.blocks() > 1000);
    }
}
