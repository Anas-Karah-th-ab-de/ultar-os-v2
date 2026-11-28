//! The state machine of one spoken request: listen for the word, record the request until
//! silence, check the speaker, hand it on, speak the answer, listen again. It also records
//! the enrolment takes and runs the Settings test. The pipeline only decides; the service owns
//! the backends and performs the effects. All time is counted in samples, so it is exact in
//! tests.
//!
//! Nothing is kept before the word: while listening the audio only passes through a short
//! utterance buffer that is dropped at the next pause; a request is recorded only after the
//! word and is dropped as soon as it was handed on.

use crate::detect::{VoicePrint, WakeWord};
use crate::mfcc::Mfcc;
use crate::vad::{Segmenter, Vad, HOP};
use ultar_bus::messages::voice::{EnrollKind, Phase, MAX_REQUEST_SECS, SAMPLE_RATE};

const RATE: usize = SAMPLE_RATE as usize;
/// Longest utterance looked at for the wake word (3 s).
pub const WAKE_MAX: usize = RATE * 3;
/// Pause that ends the request (1.2 s).
pub const END_HOPS: usize = 120;
/// How long the person has to start speaking after the word (6 s).
pub const START_TIMEOUT: usize = RATE * 6;
/// Start of the recording that is thrown away: the listening tone (0.3 s).
pub const TONE_SKIP: usize = RATE * 3 / 10;
/// Most samples of one request (15 s).
pub const MAX_REQUEST: usize = RATE * MAX_REQUEST_SECS as usize;
/// How long the Settings test waits for the word (10 s).
pub const TEST_WINDOW: usize = RATE * 10;
/// How long an enrolment take waits for the person (10 s).
pub const TAKE_WINDOW: usize = RATE * 10;
/// Shortest take of the voice sentence (1.5 s).
pub const VOICE_TAKE_MIN: usize = RATE * 3 / 2;
/// Longest take of the voice sentence (12 s).
pub const VOICE_TAKE_MAX: usize = RATE * 12;
/// Longest take of a word (3 s).
pub const WORD_TAKE_MAX: usize = WAKE_MAX;

/// What the service must do next.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// The word was heard: play the listening tone, show the mark.
    BeginRecording,
    /// An enrolment take is complete.
    Take {
        /// What is enrolled.
        kind: EnrollKind,
        /// The take.
        samples: Vec<f32>,
    },
    /// No take came in time.
    TakeMissed(EnrollKind),
    /// The Settings test heard the word.
    Heard,
    /// The Settings test heard nothing in time.
    Missed,
    /// The request is complete and was the person's voice: transcribe it.
    Transcribe(Vec<f32>),
    /// The speaker is not the person: nothing was kept, nothing runs.
    RefuseSpeaker,
    /// Nobody spoke after the word.
    GaveUp,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Wake,
    Test,
    Enrol(EnrollKind),
}

/// What the person trained.
#[derive(Default)]
struct Templates {
    wake: Option<WakeWord>,
    secret: Option<WakeWord>,
    print: Option<VoicePrint>,
}

/// The decision logic.
pub struct Pipeline {
    mfcc: Mfcc,
    templates: Templates,
    locked: bool,
    listening: bool,
    phase: Phase,
    mode: Mode,
    seg: Segmenter,
    vad: Vad,
    request: Vec<f32>,
    skipped: usize,
    waited: usize,
    silent: usize,
    spoke: bool,
    owner_ok: bool,
}

impl Pipeline {
    /// A pipeline without enrolment, idle.
    pub fn new() -> Self {
        Self {
            mfcc: Mfcc::new(),
            templates: Templates::default(),
            locked: false,
            listening: false,
            phase: Phase::Idle,
            mode: Mode::Wake,
            seg: Segmenter::new(WAKE_MAX),
            vad: Vad::new(),
            request: Vec::new(),
            skipped: 0,
            waited: 0,
            silent: 0,
            spoke: false,
            owner_ok: false,
        }
    }

    /// Sets what is enrolled (`None`: not enrolled).
    pub fn enrol(
        &mut self,
        wake: Option<WakeWord>,
        secret: Option<WakeWord>,
        print: Option<VoicePrint>,
    ) {
        self.templates.wake = wake;
        self.templates.secret = secret;
        self.templates.print = print;
    }

    /// The current phase (mirrors `voice.state.phase`).
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The feature extractor (for the service's enrolment checks).
    pub fn mfcc(&self) -> &Mfcc {
        &self.mfcc
    }

    /// The screen is locked (or not): only the secret word counts while locked.
    pub fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
    }

    /// Whether anything would wake it now (a detector for the current lock state exists).
    pub fn can_wake(&self) -> bool {
        if self.locked {
            self.templates.secret.is_some()
        } else {
            self.templates.wake.is_some() || self.templates.secret.is_some()
        }
    }

    /// The next request is accepted without the speaker check (the person confirmed by
    /// fingerprint or face).
    pub fn accept_next_speaker(&mut self) {
        self.owner_ok = true;
    }

    fn clear(&mut self) {
        self.seg.reset();
        self.vad.reset();
        self.request.clear();
        self.skipped = 0;
        self.waited = 0;
        self.silent = 0;
        self.spoke = false;
    }

    /// Back to the quiet baseline: listening for the word when it is on, idle otherwise.
    fn baseline(&mut self) {
        self.clear();
        self.mode = Mode::Wake;
        self.seg = Segmenter::new(WAKE_MAX);
        self.phase = if self.listening {
            Phase::Listening
        } else {
            Phase::Idle
        };
    }

    /// An enrolment take or the Settings test is under way: it is not interrupted.
    fn interactive(&self) -> bool {
        self.phase == Phase::Enrolling
            || (self.phase == Phase::Listening && self.mode == Mode::Test)
    }

    /// Starts listening for the word (voice control is on).
    pub fn start(&mut self) {
        self.listening = true;
        if !self.interactive() && matches!(self.phase, Phase::Idle | Phase::Listening) {
            self.baseline();
        }
    }

    /// Stops listening at once (voice control is off): everything buffered is dropped.
    pub fn stop(&mut self) {
        self.listening = false;
        if !self.interactive() {
            self.baseline();
        }
    }

    /// Gives up whatever is under way (an enrolment take, the test) and goes back to the
    /// baseline.
    pub fn cancel(&mut self) {
        self.baseline();
    }

    /// Records the next enrolment take.
    pub fn begin_enrol(&mut self, kind: EnrollKind) {
        self.clear();
        self.mode = Mode::Enrol(kind);
        let max = match kind {
            EnrollKind::Voice => VOICE_TAKE_MAX,
            _ => WORD_TAKE_MAX,
        };
        self.seg = Segmenter::new(max);
        self.phase = Phase::Enrolling;
    }

    /// Waits once for the word (the Settings test button).
    pub fn begin_test(&mut self) {
        self.clear();
        self.mode = Mode::Test;
        self.seg = Segmenter::new(WAKE_MAX);
        self.phase = Phase::Listening;
    }

    /// Feeds samples; returns the effects they led to.
    pub fn feed(&mut self, samples: &[f32]) -> Vec<Effect> {
        match self.phase {
            Phase::Listening => self.listen(samples),
            Phase::Enrolling => self.take(samples),
            Phase::Recording => self.record(samples),
            _ => Vec::new(),
        }
    }

    /// The transcription finished; `true` when there is a request to hand on.
    pub fn on_text(&mut self, empty: bool) -> bool {
        if empty {
            self.baseline();
            false
        } else {
            self.phase = Phase::Thinking;
            true
        }
    }

    /// The transcription started.
    pub fn thinking(&mut self) {
        self.phase = Phase::Thinking;
    }

    /// The helper answered; the answer is being spoken.
    pub fn answering(&mut self) {
        self.phase = Phase::Answering;
    }

    /// After the answer was spoken (or failed): listen again.
    pub fn resume(&mut self) {
        self.baseline();
    }

    fn word_matches(&self, utterance: &[f32]) -> bool {
        let wake = if self.locked {
            None
        } else {
            self.templates.wake.as_ref()
        };
        [wake, self.templates.secret.as_ref()]
            .into_iter()
            .flatten()
            .any(|d| d.matches(&self.mfcc, utterance))
    }

    fn listen(&mut self, samples: &[f32]) -> Vec<Effect> {
        let mut out = Vec::new();
        for utterance in self.seg.push(samples) {
            if !self.word_matches(&utterance) {
                continue;
            }
            if self.mode == Mode::Test {
                self.baseline();
                out.push(Effect::Heard);
            } else {
                // What came right after the word (the start of the request) is kept.
                let rest = self.seg.take_rest();
                self.clear();
                self.phase = Phase::Recording;
                out.push(Effect::BeginRecording);
                self.absorb(&rest);
            }
            return out;
        }
        if self.mode == Mode::Test {
            self.waited += samples.len();
            if self.waited >= TEST_WINDOW {
                self.baseline();
                out.push(Effect::Missed);
            }
        }
        out
    }

    fn take(&mut self, samples: &[f32]) -> Vec<Effect> {
        let Mode::Enrol(kind) = self.mode else {
            return Vec::new();
        };
        for utterance in self.seg.push(samples) {
            let fits = match kind {
                EnrollKind::Voice => utterance.len() >= VOICE_TAKE_MIN,
                _ => utterance.len() <= WORD_TAKE_MAX,
            };
            if fits {
                self.baseline();
                return vec![Effect::Take {
                    kind,
                    samples: utterance,
                }];
            }
        }
        self.waited += samples.len();
        if self.waited >= TAKE_WINDOW {
            self.baseline();
            return vec![Effect::TakeMissed(kind)];
        }
        Vec::new()
    }

    /// Adds samples to the request and follows who speaks.
    fn absorb(&mut self, samples: &[f32]) {
        self.request.extend_from_slice(samples);
        for hop in self.vad.push(samples) {
            if hop.speech {
                self.spoke = true;
                self.silent = 0;
            } else if self.spoke {
                self.silent += 1;
            } else {
                self.waited += HOP;
            }
        }
    }

    fn record(&mut self, samples: &[f32]) -> Vec<Effect> {
        let usable = if self.skipped < TONE_SKIP {
            let skip = (TONE_SKIP - self.skipped).min(samples.len());
            self.skipped += skip;
            &samples[skip..]
        } else {
            samples
        };
        if usable.is_empty() {
            return Vec::new();
        }
        self.absorb(usable);
        if !self.spoke && self.waited >= START_TIMEOUT {
            self.baseline();
            return vec![Effect::GaveUp];
        }
        let finished = (self.spoke && self.silent >= END_HOPS) || self.request.len() >= MAX_REQUEST;
        if !finished {
            return Vec::new();
        }
        let request = std::mem::take(&mut self.request);
        let skip_check = std::mem::take(&mut self.owner_ok);
        if let Some(print) = &self.templates.print {
            if !skip_check && !print.accepts(&self.mfcc, &request) {
                self.baseline();
                return vec![Effect::RefuseSpeaker];
            }
        }
        self.phase = Phase::Thinking;
        vec![Effect::Transcribe(request)]
    }
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
