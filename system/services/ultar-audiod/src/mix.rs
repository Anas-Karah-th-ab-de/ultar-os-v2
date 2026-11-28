//! The mixer of the sound server: any number of play streams into one 48 kHz stereo
//! stream, with a volume per stream, the priority of the classes (a call ducks media, a
//! ring and a prompt duck it less), a soft limiter that can never wrap around, and fades
//! at every start, stop and underrun so that nothing clicks.
//!
//! Pure logic without sockets or devices: the engine feeds frames in and takes mixed
//! frames out.

use std::collections::VecDeque;
use ultar_audio_stream::Class;

/// Frames a stream may hold (100 ms at 48 kHz): beyond that the engine stops reading the
/// client, which then sees "would block".
pub const MAX_FRAMES: usize = 4800;
/// Frames a stream needs before it starts to sound (20 ms): a start without a cushion
/// would break up at once. A client that runs dry gets a bigger cushion (10 ms more each
/// time, up to [MAX_PREFILL]).
pub const PREFILL: usize = 960;
/// Largest cushion (80 ms).
pub const MAX_PREFILL: usize = 3840;
/// Frames of a fade at a start, an end and an underrun (2 ms).
pub const FADE_FRAMES: usize = 96;
/// How long a class counts as active after its last sound (300 ms).
pub const HANG_FRAMES: usize = 14_400;
/// Gain of media under a call (−15 dB).
pub const DUCK_CALL: f32 = 0.177_827_94;
/// Gain of media under a ring (−12 dB).
pub const DUCK_RING: f32 = 0.251_188_64;
/// Gain of media under a prompt (−9 dB).
pub const DUCK_PROMPT: f32 = 0.354_813_4;
/// Frames the duck takes to go down (10 ms) and to come back (250 ms).
const DUCK_DOWN: f32 = 480.0;
const DUCK_UP: f32 = 12_000.0;
/// Level from which the limiter bends the sum.
const KNEE: f32 = 0.8;

type Frame = (f32, f32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Starved,
    Playing,
}

struct Voice {
    id: u64,
    class: Class,
    gain: f32,
    buf: VecDeque<Frame>,
    state: State,
    last: Frame,
    fade: f32,
    ended: bool,
    prefill: usize,
    /// Frames since the stream last brought sound.
    quiet: usize,
    underruns: u64,
}

/// The mixer.
pub struct Mix {
    voices: Vec<Voice>,
    /// Gain per class (index = [`Class::byte`]), moving towards its target.
    duck: [f32; 4],
}

impl Default for Mix {
    fn default() -> Self {
        Self::new()
    }
}

/// The soft limiter: unchanged up to the knee, then bent towards (never over) 1.0.
pub fn limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        let room = 1.0 - KNEE;
        x.signum() * (KNEE + room * ((a - KNEE) / room).tanh())
    }
}

impl Mix {
    /// An empty mixer.
    pub fn new() -> Self {
        Self {
            voices: Vec::new(),
            duck: [1.0; 4],
        }
    }

    /// Opens a play stream with `volume` per mille.
    pub fn add(&mut self, id: u64, class: Class, volume: u16) {
        self.voices.push(Voice {
            id,
            class,
            gain: f32::from(volume) / 1000.0,
            buf: VecDeque::new(),
            state: State::Starved,
            last: (0.0, 0.0),
            fade: 0.0,
            ended: false,
            prefill: PREFILL,
            quiet: HANG_FRAMES,
            underruns: 0,
        });
    }

    fn voice(&mut self, id: u64) -> Option<&mut Voice> {
        self.voices.iter_mut().find(|v| v.id == id)
    }

    /// Changes the volume of a stream (per mille).
    pub fn set_volume(&mut self, id: u64, volume: u16) {
        if let Some(v) = self.voice(id) {
            v.gain = f32::from(volume) / 1000.0;
        }
    }

    /// Frames the stream can take now.
    pub fn room(&self, id: u64) -> usize {
        self.voices
            .iter()
            .find(|v| v.id == id)
            .map_or(0, |v| MAX_FRAMES.saturating_sub(v.buf.len()))
    }

    /// Frames waiting in the stream.
    pub fn buffered(&self, id: u64) -> usize {
        self.voices
            .iter()
            .find(|v| v.id == id)
            .map_or(0, |v| v.buf.len())
    }

    /// Gives the stream frames (as many as fit are kept).
    pub fn push(&mut self, id: u64, frames: &[Frame]) {
        if let Some(v) = self.voice(id) {
            let room = MAX_FRAMES.saturating_sub(v.buf.len());
            v.buf.extend(frames.iter().take(room));
            if !frames.is_empty() {
                v.quiet = 0;
            }
        }
    }

    /// The client has no more sound; the stream ends when it is played out.
    pub fn end(&mut self, id: u64) {
        if let Some(v) = self.voice(id) {
            v.ended = true;
        }
    }

    /// Takes a stream away at once (the client died).
    pub fn remove(&mut self, id: u64) {
        self.voices.retain(|v| v.id != id);
    }

    /// Streams in the mixer.
    pub fn len(&self) -> usize {
        self.voices.len()
    }

    /// No stream.
    pub fn is_empty(&self) -> bool {
        self.voices.is_empty()
    }

    /// Whether anything is, or just was, sounding (the card needs frames).
    pub fn sounding(&self) -> bool {
        self.voices
            .iter()
            .any(|v| !v.buf.is_empty() || v.fade > 0.0)
    }

    /// Times a stream ran dry while it was playing (a client too slow).
    pub fn underruns(&self, id: u64) -> u64 {
        self.voices
            .iter()
            .find(|v| v.id == id)
            .map_or(0, |v| v.underruns)
    }

    /// Current gain of a class (1.0: not ducked).
    pub fn duck_of(&self, class: Class) -> f32 {
        self.duck[usize::from(class.byte())]
    }

    fn active(&self, class: Class) -> bool {
        self.voices
            .iter()
            .any(|v| v.class == class && (!v.buf.is_empty() || v.quiet < HANG_FRAMES))
    }

    fn targets(&self) -> [f32; 4] {
        let mut t = [1.0f32; 4];
        let media = usize::from(Class::Media.byte());
        if self.active(Class::Call) {
            t[media] = DUCK_CALL;
        } else if self.active(Class::Ring) {
            t[media] = DUCK_RING;
        } else if self.active(Class::Prompt) {
            t[media] = DUCK_PROMPT;
        }
        t
    }

    /// Mixes `out.len() / 2` frames (interleaved stereo) and returns the ids of the streams
    /// that ended and are played out (they are gone from the mixer).
    pub fn mix(&mut self, out: &mut [f32]) -> Vec<u64> {
        let frames = out.len() / 2;
        out.fill(0.0);
        let targets = self.targets();
        let step = 1.0 / FADE_FRAMES as f32;
        let mut done = Vec::new();
        for i in 0..frames {
            for (c, d) in self.duck.iter_mut().enumerate() {
                let target = targets[c];
                if *d > target {
                    *d = (*d - (1.0 - DUCK_CALL) / DUCK_DOWN).max(target);
                } else if *d < target {
                    *d = (*d + (1.0 - DUCK_CALL) / DUCK_UP).min(target);
                }
            }
            let (mut l, mut r) = (0.0f32, 0.0f32);
            for v in &mut self.voices {
                let class_gain = self.duck[usize::from(v.class.byte())];
                if v.state == State::Starved && (v.buf.len() >= v.prefill || (v.ended && !v.buf.is_empty())) {
                    v.state = State::Playing;
                }
                let (a, b) = match (v.state, v.buf.front().copied()) {
                    (State::Playing, Some(f)) => {
                        v.buf.pop_front();
                        v.fade = (v.fade + step).min(1.0);
                        v.last = f;
                        (f.0 * v.fade, f.1 * v.fade)
                    }
                    (State::Playing, None) => {
                        if !v.ended {
                            v.underruns += 1;
                            v.prefill = (v.prefill + 480).min(MAX_PREFILL);
                        }
                        v.state = State::Starved;
                        v.fade = (v.fade - step).max(0.0);
                        (v.last.0 * v.fade, v.last.1 * v.fade)
                    }
                    (State::Starved, _) => {
                        v.fade = (v.fade - step).max(0.0);
                        (v.last.0 * v.fade, v.last.1 * v.fade)
                    }
                };
                if v.buf.is_empty() && v.quiet < usize::MAX - 1 {
                    v.quiet += 1;
                }
                l += a * v.gain * class_gain;
                r += b * v.gain * class_gain;
            }
            out[2 * i] = limit(l);
            out[2 * i + 1] = limit(r);
        }
        done.extend(self.reap());
        done
    }

    /// Takes away the streams that ended and are played out; returns their ids. (A stream
    /// that has no sound left and no fade to finish needs no mixing to go.)
    pub fn reap(&mut self) -> Vec<u64> {
        let mut gone = Vec::new();
        self.voices.retain(|v| {
            let over = v.ended && v.buf.is_empty() && v.fade <= 0.0;
            if over {
                gone.push(v.id);
            }
            !over
        });
        gone
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mixes `frames` in periods of 240 frames (5 ms), like the engine does.
    fn run(m: &mut Mix, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        for chunk in out.chunks_mut(480) {
            m.mix(chunk);
        }
        out
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0, |a, v| a.max(v.abs()))
    }

    /// Largest step between two neighbouring samples of one channel: a click is a big one.
    fn jump(x: &[f32]) -> f32 {
        x.chunks_exact(2)
            .zip(x.chunks_exact(2).skip(1))
            .fold(0.0, |a, (p, q)| a.max((q[0] - p[0]).abs()))
    }

    #[test]
    fn streams_are_summed_and_each_has_its_volume() {
        let mut m = Mix::new();
        m.add(1, Class::Media, 1000);
        m.add(2, Class::Media, 500);
        let one = vec![(0.2, 0.2); 4000];
        m.push(1, &one);
        m.push(2, &one);
        let out = run(&mut m, 3000);
        // after the fade-in 0.2 + 0.5 * 0.2 = 0.3
        assert!((out[2 * 2500] - 0.3).abs() < 1e-4, "{}", out[2 * 2500]);
        assert!((out[2 * 2500 + 1] - 0.3).abs() < 1e-4);
        m.set_volume(2, 0);
        let out = run(&mut m, 100);
        assert!((out[0] - 0.2).abs() < 1e-4);
    }

    #[test]
    fn many_streams_never_wrap_and_stay_below_one() {
        let mut m = Mix::new();
        for id in 0..16 {
            m.add(id, Class::Media, 2000);
            m.push(id, &vec![(0.9, -0.9); 4000]);
        }
        let out = run(&mut m, 4000);
        assert!(peak(&out) <= 1.0, "peak {}", peak(&out));
        assert!(out[2 * 3000] > 0.99 && out[2 * 3000 + 1] < -0.99, "driven hard");
        // below the knee the sum is exact
        assert_eq!(limit(0.5), 0.5);
        assert_eq!(limit(-0.8), -0.8);
        assert!(limit(1.0) < 1.0 && limit(100.0) <= 1.0 && limit(-100.0) >= -1.0);
        assert!(limit(0.9) > 0.8 && limit(0.9) < limit(1.2), "monotonic");
    }

    #[test]
    fn a_start_and_an_underrun_fade_instead_of_clicking() {
        let mut m = Mix::new();
        m.add(1, Class::Media, 1000);
        // loud constant sound, but less than the cushion: nothing sounds yet
        m.push(1, &vec![(0.8, 0.8); 100]);
        assert_eq!(peak(&run(&mut m, 200)), 0.0);
        m.push(1, &vec![(0.8, 0.8); 1000]);
        let start = run(&mut m, 300);
        assert!(jump(&start) < 0.8 / FADE_FRAMES as f32 * 1.2, "start {}", jump(&start));
        // the client stops: the sound runs out and fades from where it was
        let out = run(&mut m, 2000);
        assert!(jump(&out) < 0.8 / FADE_FRAMES as f32 * 1.2, "underrun {}", jump(&out));
        assert!(out[2 * 1999].abs() < 1e-6, "silent at the end");
        assert_eq!(m.underruns(1), 1);
        assert!(!m.sounding());
    }

    #[test]
    fn a_call_ducks_media_and_media_returns_afterwards() {
        let mut m = Mix::new();
        m.add(1, Class::Media, 1000);
        m.add(2, Class::Call, 1000);
        m.add(3, Class::Prompt, 1000);
        m.push(1, &vec![(0.4, 0.4); MAX_FRAMES]);
        let before = run(&mut m, 2000);
        assert!((before[2 * 1999] - 0.4).abs() < 1e-4);
        m.push(2, &vec![(0.0, 0.0); 3000]);
        run(&mut m, 1500);
        m.push(1, &vec![(0.4, 0.4); MAX_FRAMES]);
        let ducked = run(&mut m, 500);
        let level = 20.0 * (ducked[2 * 499] / 0.4).log10();
        assert!((level + 15.0).abs() < 0.2, "media at {level} dB under a call");
        // a prompt during the call is not ducked, and does not lift the duck
        m.push(3, &vec![(0.3, 0.3); 4000]);
        let with_prompt = run(&mut m, 1000);
        assert!((with_prompt[2 * 999] - (0.3 + 0.4 * DUCK_CALL)).abs() < 0.01);
        // the call ends: media comes back within the release time
        m.end(2);
        run(&mut m, 30_000);
        m.push(1, &vec![(0.4, 0.4); 2000]);
        let after = run(&mut m, 1000);
        assert!((after[2 * 999] - 0.4).abs() < 0.01, "back at {}", after[2 * 999]);
        assert!(m.duck_of(Class::Media) > 0.999);
    }

    #[test]
    fn a_ring_and_a_prompt_duck_media_less_than_a_call() {
        let mut m = Mix::new();
        m.add(1, Class::Media, 1000);
        m.add(2, Class::Ring, 1000);
        m.push(1, &vec![(0.4, 0.4); MAX_FRAMES]);
        m.push(2, &vec![(0.1, 0.1); 3000]);
        run(&mut m, 1000);
        assert!((m.duck_of(Class::Media) - DUCK_RING).abs() < 1e-3);
        let mut p = Mix::new();
        p.add(1, Class::Media, 1000);
        p.add(2, Class::Prompt, 1000);
        p.push(1, &vec![(0.4, 0.4); MAX_FRAMES]);
        p.push(2, &vec![(0.1, 0.1); 3000]);
        run(&mut p, 1000);
        assert!((p.duck_of(Class::Media) - DUCK_PROMPT).abs() < 1e-3);
        assert!(DUCK_CALL < DUCK_RING && DUCK_RING < DUCK_PROMPT);
    }

    #[test]
    fn ended_streams_leave_after_they_are_played_out_and_room_is_bounded() {
        let mut m = Mix::new();
        m.add(7, Class::Media, 1000);
        assert_eq!(m.room(7), MAX_FRAMES);
        m.push(7, &vec![(0.1, 0.1); MAX_FRAMES + 500]);
        assert_eq!(m.buffered(7), MAX_FRAMES, "what does not fit is not kept");
        assert_eq!(m.room(7), 0);
        m.end(7);
        let mut out = vec![0.0; 2 * 1000];
        let mut gone = Vec::new();
        for _ in 0..8 {
            gone.extend(m.mix(&mut out));
        }
        assert_eq!(gone, vec![7]);
        assert!(m.is_empty() && m.len() == 0);
        // a short clip (less than the cushion) that ended still plays
        m.add(8, Class::Prompt, 1000);
        m.push(8, &vec![(0.5, 0.5); 200]);
        m.end(8);
        assert!(peak(&run(&mut m, 300)) > 0.4);
        m.remove(8);
        assert!(m.is_empty());
        assert_eq!(m.room(99), 0);
    }
}
