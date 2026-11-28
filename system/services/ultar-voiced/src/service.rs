//! The bus side of the voice service: `voice.*` calls from Settings and the lock screen, the
//! microphone chunks of the audio service, and the state it publishes.
//!
//! The audio goes only through `ultar-audiod` (`audio.capture`, `audio.mic`, `audio.play`);
//! the request goes to the helper as `agent.ask_voice`. Voice control is off until the person
//! switches it on; while it is off the microphone stream is not even asked for.

use crate::audio::{f32_to_pcm16, pcm16_to_f32};
use crate::engine::{Speaker, Transcriber};
use crate::job::{self, End, Setup, Work};
use crate::models::ModelCatalog;
use crate::parts::{Errors, Mic, Trial};
use crate::pipeline::{Effect, Pipeline};
use crate::state::{Enrollment, Settings, Store, DEFAULT_WAKE_WORD};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_bus::messages::voice::{
    Capture, EnrollKind, MicChunk, Phase, Ready, TestResult, Training, VoiceError, VoiceState,
    MAX_CHUNK_SAMPLES, SAMPLE_RATE, SERVICE, TAKES,
};
use ultar_bus::{IncomingRequest, RawEvent, Topic};
use ultar_service::log::{Level, Record};
use ultar_svckit::{Bus, Service};

/// How often the service looks after itself (idle servers, the microphone, the confirmation).
pub const TICK: Duration = Duration::from_secs(1);
const CALL_TIME: Duration = Duration::from_secs(5);
/// How long the person has to confirm by fingerprint or face after a refused voice.
pub const VERIFY_WINDOW: Duration = Duration::from_secs(60);
/// The microphone stream counts as lost after this long without a chunk.
const MIC_LOST: Duration = Duration::from_secs(5);
/// The Settings test gives up after this long even without audio.
const TEST_WALL: Duration = Duration::from_secs(14);

fn slot(kind: EnrollKind) -> usize {
    match kind {
        EnrollKind::Wake => 0,
        EnrollKind::Secret => 1,
        EnrollKind::Voice => 2,
    }
}

/// The voice service.
pub struct VoiceService {
    store: Store,
    settings: Settings,
    enrol: Enrollment,
    pending: [Vec<Vec<f32>>; 3],
    pipe: Pipeline,
    stt: Arc<dyn Transcriber>,
    tts: Arc<dyn Speaker>,
    models: ModelCatalog,
    locked: bool,
    busy: bool,
    mic: Mic,
    verify_until: Option<Instant>,
    test: Trial,
    errors: Errors,
    last: Option<VoiceState>,
}

#[path = "service_calls.rs"]
mod calls;

impl VoiceService {
    /// A service with its state in `store` and its backends.
    pub fn new(
        store: Store,
        stt: Arc<dyn Transcriber>,
        tts: Arc<dyn Speaker>,
        models: ModelCatalog,
    ) -> Self {
        let (settings, enrol) = store.load();
        let mut pipe = Pipeline::new();
        pipe.enrol(
            enrol.wake.clone(),
            enrol.secret.clone(),
            enrol.print.clone(),
        );
        Self {
            store,
            settings,
            enrol,
            pending: [Vec::new(), Vec::new(), Vec::new()],
            pipe,
            stt,
            tts,
            models,
            locked: false,
            busy: false,
            mic: Mic::default(),
            verify_until: None,
            test: Trial::default(),
            errors: Errors::default(),
            last: None,
        }
    }

    fn stt_ready(&self) -> bool {
        self.stt.installed(&self.settings.model)
    }

    /// The state as the surfaces see it.
    pub fn state(&self) -> VoiceState {
        let training = |kind: EnrollKind, enrolled: bool| Training {
            enrolled,
            clips: if enrolled {
                TAKES
            } else {
                u8::try_from(self.pending[slot(kind)].len()).unwrap_or(TAKES)
            },
        };
        let enrolled = self.enrol.wake.is_some() || self.enrol.secret.is_some();
        let mut error = self.errors.take.or(self.errors.job);
        if error.is_none() && self.settings.enabled {
            error = if !self.stt_ready() {
                Some(VoiceError::NoStt)
            } else if !enrolled {
                Some(VoiceError::NotEnrolled)
            } else if !self.mic.ready && self.mic.on {
                Some(VoiceError::NoAudio)
            } else if !self.tts.installed() {
                Some(VoiceError::NoTts)
            } else {
                None
            };
        }
        VoiceState {
            enabled: self.settings.enabled,
            phase: self.pipe.phase(),
            wake_word: DEFAULT_WAKE_WORD.to_owned(),
            wake: training(EnrollKind::Wake, self.enrol.wake.is_some()),
            secret: training(EnrollKind::Secret, self.enrol.secret.is_some()),
            voice: training(EnrollKind::Voice, self.enrol.print.is_some()),
            language: self.settings.language.clone(),
            model: self.settings.model.clone(),
            models: self.models.list(),
            ready: Ready {
                stt: self.stt_ready(),
                tts: self.tts.installed(),
                mic: self.mic.ready,
            },
            locked: self.locked,
            verify_needed: self.verify_until.is_some(),
            test: self.test.result,
            error,
        }
    }

    fn publish(&mut self, bus: &Bus<'_, Self>) {
        let now = self.state();
        if self.last.as_ref() != Some(&now) {
            bus.retain(&now);
            self.last = Some(now);
        }
    }

    /// Whether the microphone stream is needed now.
    fn want_capture(&self) -> bool {
        match self.pipe.phase() {
            Phase::Enrolling => true,
            _ => {
                self.test.result == TestResult::Waiting
                    || (self.settings.enabled
                        && self.stt_ready()
                        && self.pipe.can_wake()
                        && self.has_wake_detector())
            }
        }
    }

    fn has_wake_detector(&self) -> bool {
        self.enrol.wake.is_some() || self.enrol.secret.is_some()
    }

    /// Asks the audio service to start or stop the stream when that differs from what it was
    /// told (and asks again every few seconds while it does not deliver).
    fn sync_capture(&mut self, bus: &Bus<'_, Self>) {
        let want = self.want_capture();
        let retry =
            want && !self.mic.ready && self.mic.asked.is_none_or(|t| t.elapsed() >= MIC_LOST);
        if want == self.mic.on && !retry {
            return;
        }
        self.mic.on = want;
        self.mic.asked = Some(Instant::now());
        if !want {
            self.mic.ready = false;
            self.mic.last_chunk = None;
        }
        let asked = bus.later(move |client| {
            let ok = client
                .call(&Capture { on: want }, CALL_TIME)
                .map(|r| r.ok)
                .unwrap_or(false);
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                if service.mic.on == want {
                    service.mic.ready = want && ok;
                    if service.mic.ready {
                        service.mic.last_chunk = Some(Instant::now());
                    }
                    service.publish(bus);
                }
            })
        });
        if !asked {
            self.mic.asked = None;
        }
    }

    /// Starts listening or stops, after a change of what is on or enrolled.
    fn apply(&mut self, bus: &Bus<'_, Self>) {
        self.pipe.enrol(
            self.enrol.wake.clone(),
            self.enrol.secret.clone(),
            self.enrol.print.clone(),
        );
        self.pipe.set_locked(self.locked);
        let listen = self.settings.enabled && self.stt_ready() && self.has_wake_detector();
        if listen {
            self.pipe.start();
        } else {
            self.pipe.stop();
        }
        self.sync_capture(bus);
        self.publish(bus);
    }

    fn save_enrollment(&self, bus: &Bus<'_, Self>) {
        if !self.store.save_enrollment(&self.enrol) {
            bus.log(Record::new(Level::Warn, "voiced.not_saved"));
        }
    }

    fn save_settings(&self, bus: &Bus<'_, Self>) {
        if !self.store.save_settings(&self.settings) {
            bus.log(Record::new(Level::Warn, "voiced.not_saved"));
        }
    }

    fn start_job(&mut self, work: Work, bus: &Bus<'_, Self>) {
        let (stt, tts) = (self.stt.clone(), self.tts.clone());
        let (language, model) = (self.settings.language.clone(), self.settings.model.clone());
        let poster = bus.poster();
        let started = bus.later(move |client| {
            let setup = Setup {
                language: &language,
                model: &model,
                client,
                stt: stt.as_ref(),
                tts: tts.as_ref(),
            };
            let answering = || {
                let _ = poster.post(Box::new(|service: &mut Self, bus: &Bus<'_, Self>| {
                    service.pipe.answering();
                    service.publish(bus);
                }));
            };
            let end = job::run(&setup, work, &answering);
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| service.job_done(end, bus))
        });
        if started {
            self.busy = true;
        } else {
            self.pipe.resume();
        }
    }

    fn job_done(&mut self, end: End, bus: &Bus<'_, Self>) {
        self.busy = false;
        self.errors.job = end.error;
        self.pipe.resume();
        self.publish(bus);
    }

    fn effects(&mut self, effects: Vec<Effect>, bus: &Bus<'_, Self>) {
        for effect in effects {
            match effect {
                Effect::BeginRecording => {
                    let _ = bus.later(|client| {
                        let _ = client.call(&crate::speech::tone(), CALL_TIME);
                        Box::new(|_: &mut Self, _: &Bus<'_, Self>| {})
                    });
                }
                Effect::Take { kind, samples } => self.take(kind, samples, bus),
                Effect::TakeMissed(_) => self.errors.take = Some(VoiceError::TakeMissed),
                Effect::Heard => self.test.result = TestResult::Heard,
                Effect::Missed => self.test.result = TestResult::Missed,
                Effect::Transcribe(request) => {
                    let pcm = f32_to_pcm16(&request);
                    self.start_job(
                        Work::Request {
                            pcm,
                            locked: self.locked,
                        },
                        bus,
                    );
                }
                Effect::RefuseSpeaker => {
                    self.verify_until = Some(Instant::now() + VERIFY_WINDOW);
                    self.pipe.thinking();
                    let said = vec!["voice.say.not_standard_user", "voice.say.confirm_owner"];
                    self.start_job(Work::Say(said), bus);
                }
                Effect::GaveUp => {}
            }
        }
        self.settle(bus);
    }

    /// After anything that changed the phase: the stream, the state.
    fn settle(&mut self, bus: &Bus<'_, Self>) {
        if self.test.result != TestResult::Waiting
            && self.pipe.phase() == Phase::Idle
            && !self.settings.enabled
        {
            self.pipe.stop();
        }
        self.sync_capture(bus);
        self.publish(bus);
    }
}

impl Service for VoiceService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn subscriptions(&self) -> Vec<&'static str> {
        vec![MicChunk::TOPIC]
    }

    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        bus.log(Record::new(Level::Info, "voiced.start"));
        self.apply(bus);
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        self.calls(request, bus);
    }

    fn event(&mut self, event: RawEvent, bus: &Bus<'_, Self>) {
        // Nothing is looked at unless the stream was asked for: a chunk that nobody wanted is
        // dropped unread.
        if !self.mic.on {
            return;
        }
        let Ok(chunk) = event.decode::<MicChunk>() else {
            return;
        };
        if chunk.rate != SAMPLE_RATE
            || chunk.samples.is_empty()
            || chunk.samples.len() > MAX_CHUNK_SAMPLES
        {
            return;
        }
        self.mic.last_chunk = Some(Instant::now());
        if !self.mic.ready {
            self.mic.ready = true;
        }
        let effects = self.pipe.feed(&pcm16_to_f32(&chunk.samples));
        if effects.is_empty() {
            self.publish(bus);
        } else {
            self.effects(effects, bus);
        }
    }

    fn tick(&mut self, bus: &Bus<'_, Self>) {
        self.stt.idle_check();
        if self.verify_until.is_some_and(|t| Instant::now() >= t) {
            self.verify_until = None;
        }
        if self.mic.on
            && self.mic.ready
            && self.mic.last_chunk.is_some_and(|t| t.elapsed() >= MIC_LOST)
        {
            self.mic.ready = false;
        }
        // The test also ends when no audio comes at all (the pipeline counts samples).
        if self.test.result == TestResult::Waiting
            && self.test.until.is_some_and(|t| Instant::now() >= t)
        {
            self.test.result = TestResult::Missed;
            self.pipe.cancel();
        }
        self.sync_capture(bus);
        self.publish(bus);
    }
}
