//! The state machine of one spoken request: listen → record → transcribe → ask → answer →
//! speak. The pipeline decides; the service owns the backends and performs the effects.

use crate::audio;
use crate::detect::{VoicePrint, WakeWord};
use crate::mfcc::Mfcc;
use ultar_bus::messages::voice::Phase;

/// Samples of the wake-word window (0.8 s at 16 kHz).
pub const WAKE_WINDOW: usize = 12_800;
/// Silence this long ends the request (1.5 s).
pub const STOP_SILENCE: usize = 24_000;
/// Most samples of one request (15 s).
pub const MAX_REQUEST: usize = 240_000;

/// What the service must do next.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Nothing to do.
    None,
    /// Publish the listening mark and play the short sound.
    BeginListening,
    /// The word was heard: publish the recording mark and play the sound.
    BeginRecording,
    /// The request is complete: transcribe it.
    Transcribe(Vec<f32>),
    /// The speaker is not the person: refuse, do nothing else.
    RefuseSpeaker,
    /// Ask the helper by voice.
    Ask(String),
    /// Speak the answer aloud.
    Speak(String),
}

/// The decision logic; the MFCC extractor is built once per pipeline.
pub struct Pipeline {
    mfcc: Mfcc,
    wake: Option<WakeWord>,
    secret: Option<WakeWord>,
    print: Option<VoicePrint>,
    locked: bool,
    phase: Phase,
    ring: Vec<f32>,
    request: Vec<f32>,
    silent: usize,
}

impl Pipeline {
    /// A pipeline from the enrolled templates; `None` disables that part.
    pub fn new(
        wake: Option<WakeWord>,
        secret: Option<WakeWord>,
        print: Option<VoicePrint>,
        locked: bool,
    ) -> Self {
        Self {
            mfcc: Mfcc::new(),
            wake,
            secret,
            print,
            locked,
            phase: Phase::Idle,
            ring: Vec::new(),
            request: Vec::new(),
            silent: 0,
        }
    }

    /// The current phase (mirrors `voice.state.phase`).
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The screen is locked now: only the secret word wakes the helper.
    pub fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
    }

    /// Starts (or restarts) listening; the first `BeginListening` marks the state.
    pub fn begin(&mut self) -> Effect {
        if self.phase == Phase::Idle {
            self.phase = Phase::Listening;
            self.ring.clear();
            Effect::BeginListening
        } else {
            Effect::None
        }
    }

    /// Feeds one chunk of audio; returns the effects of this chunk.
    pub fn feed(&mut self, samples: &[f32]) -> Vec<Effect> {
        match self.phase {
            Phase::Listening => self.listen(samples),
            Phase::Recording => self.record(samples),
            _ => Vec::new(),
        }
    }

    /// The transcription finished (`None`: the request was silent or unspoken).
    pub fn on_text(&mut self, text: String) -> Vec<Effect> {
        let text = text.trim().to_owned();
        if text.is_empty() {
            self.phase = Phase::Idle;
            vec![Effect::None]
        } else {
            self.phase = Phase::Thinking;
            vec![Effect::Ask(text)]
        }
    }

    /// The helper answered; speak it.
    pub fn on_answer(&mut self, text: String) -> Vec<Effect> {
        self.phase = Phase::Answering;
        vec![Effect::Speak(text)]
    }

    /// After the answer was spoken (or the speech failed): listen again.
    pub fn resume(&mut self) -> Effect {
        if self.phase == Phase::Answering || self.phase == Phase::Idle {
            self.phase = Phase::Listening;
            self.ring.clear();
            Effect::BeginListening
        } else {
            Effect::None
        }
    }

    fn listen(&mut self, samples: &[f32]) -> Vec<Effect> {
        self.ring.extend_from_slice(samples);
        let drop = self.ring.len().saturating_sub(WAKE_WINDOW);
        if drop > 0 {
            self.ring.drain(..drop);
        }
        let detector = if self.locked {
            self.secret.as_ref()
        } else {
            self.wake.as_ref()
        };
        let Some(detector) = detector else {
            return Vec::new();
        };
        if detector.matches(&self.mfcc, &self.ring) {
            self.phase = Phase::Recording;
            self.request.clear();
            self.silent = 0;
            vec![Effect::BeginRecording]
        } else {
            Vec::new()
        }
    }

    fn record(&mut self, samples: &[f32]) -> Vec<Effect> {
        if audio::is_silent(samples) {
            self.silent += samples.len();
        } else {
            self.silent = 0;
        }
        self.request.extend_from_slice(samples);
        if self.silent < STOP_SILENCE && self.request.len() < MAX_REQUEST {
            return Vec::new();
        }
        let taken = std::mem::take(&mut self.request);
        self.phase = Phase::Thinking;
        // The speaker check runs on the recorded request; a stranger is refused.
        if let Some(print) = &self.print {
            if !print.accepts(&self.mfcc, &taken) {
                self.phase = Phase::Idle;
                return vec![Effect::RefuseSpeaker];
            }
        }
        vec![Effect::Transcribe(taken)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::{VoicePrint, WakeWord};
    use crate::mfcc::Mfcc;
    use std::f32::consts::PI;

    const CHUNK: usize = 160;

    /// A synthetic "word": tone bursts around `base` Hz, tweaked by `seed`.
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

    /// Feeds a whole buffer in 10 ms chunks; returns whether any effect matched `check`.
    fn feed(pipe: &mut Pipeline, samples: &[f32], check: &dyn Fn(&Effect) -> bool) -> bool {
        let mut hit = false;
        for at in (0..samples.len()).step_by(CHUNK) {
            for e in pipe.feed(&samples[at..(at + CHUNK).min(samples.len())].to_vec()) {
                hit |= check(&e);
            }
        }
        hit
    }

    /// Feeds `n` chunks of silence.
    fn silence(pipe: &mut Pipeline, chunks: usize) -> Vec<Effect> {
        let mut out = Vec::new();
        for _ in 0..chunks {
            out.extend(pipe.feed(&vec![0.0f32; CHUNK]));
        }
        out
    }

    fn is_recording(e: &Effect) -> bool {
        matches!(e, Effect::BeginRecording)
    }
    fn is_transcribe(e: &Effect) -> bool {
        matches!(e, Effect::Transcribe(_))
    }
    fn is_refuse(e: &Effect) -> bool {
        matches!(e, Effect::RefuseSpeaker)
    }

    #[test]
    fn a_full_request_flows_from_wake_to_ask() {
        let mfcc = Mfcc::new();
        let wake = WakeWord::from_clips(&mfcc, &[word(440.0, 0.0, 16_000)]).unwrap();
        let mut pipe = Pipeline::new(Some(wake), None, None, false);
        assert_eq!(pipe.begin(), Effect::BeginListening);
        assert!(feed(&mut pipe, &word(440.0, 1.0, 16_000), &is_recording));
        let effects = silence(&mut pipe, STOP_SILENCE / CHUNK + 3);
        assert!(effects.iter().any(is_transcribe));
        assert_eq!(
            pipe.on_text("افتح الإعدادات".to_owned()),
            vec![Effect::Ask("افتح الإعدادات".to_owned())]
        );
    }

    #[test]
    fn locked_means_only_the_secret_word_counts() {
        let mfcc = Mfcc::new();
        let wake = WakeWord::from_clips(&mfcc, &[word(440.0, 0.0, 16_000)]).unwrap();
        let secret = WakeWord::from_clips(&mfcc, &[word(2_000.0, 0.0, 16_000)]).unwrap();
        let mut pipe = Pipeline::new(Some(wake), Some(secret), None, true);
        pipe.begin();
        // The ordinary wake word does nothing while locked.
        assert!(!feed(&mut pipe, &word(440.0, 1.0, 16_000), &is_recording));
        // The secret word does.
        assert!(feed(&mut pipe, &word(2_000.0, 1.0, 16_000), &is_recording));
    }

    #[test]
    fn a_stranger_is_refused_while_the_person_is_accepted() {
        let mfcc = Mfcc::new();
        let wake = WakeWord::from_clips(&mfcc, &[word(440.0, 0.0, 16_000)]).unwrap();
        let print = VoicePrint::from_clips(&mfcc, &[word(600.0, 0.0, 16_000)]).unwrap();
        let mut pipe = Pipeline::new(Some(wake), None, Some(print), false);
        pipe.begin();
        assert!(feed(&mut pipe, &word(440.0, 0.0, 16_000), &is_recording));
        // The person's voice passes.
        let effects = {
            let mut p = Pipeline::new(Some(wake.clone()), None, Some(print.clone()), false);
            p.begin();
            feed(&mut p, &word(440.0, 0.0, 16_000), &is_recording);
            p.record(&word(600.0, 5.0, 16_000));
            silence(&mut p, STOP_SILENCE / CHUNK + 3)
        };
        assert!(effects.iter().any(is_transcribe) && !effects.iter().any(is_refuse));
        // A stranger is refused.
        let effects = {
            let mut p = Pipeline::new(Some(wake), None, Some(print), false);
            p.begin();
            feed(&mut p, &word(440.0, 0.0, 16_000), &is_recording);
            p.record(&word(3_000.0, 0.0, 16_000));
            silence(&mut p, STOP_SILENCE / CHUNK + 3)
        };
        assert!(effects.iter().any(is_refuse));
    }

    #[test]
    fn empty_transcript_goes_back_to_idle_and_resume_listens_again() {
        let mut pipe = Pipeline::new(None, None, None, false);
        assert_eq!(pipe.on_text("   ".to_owned()), vec![Effect::None]);
        assert_eq!(pipe.phase(), Phase::Idle);
        assert_eq!(pipe.resume(), Effect::BeginListening);
        assert_eq!(pipe.phase(), Phase::Listening);
    }
}