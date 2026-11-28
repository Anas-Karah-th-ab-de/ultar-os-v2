//! Contract of the voice control service (`ultar-voiced`): the person says the wake word,
//! the helper hears the request and answers aloud. Everything stays on the device.
//!
//! Voice control is **off** until the person switches it on. The service owns no
//! microphone and no loudspeaker of its own: it hears and speaks only through
//! `ultar-audiod` ([`MicChunk`], [`Capture`], [`Play`]). While on, the audio is analysed in
//! memory for the wake word and thrown away; a request is recorded only after the wake
//! word. Nothing is stored but the person's enrolment data (sealed in the service's state
//! directory). While it listens it publishes [`VoiceState`] so the shell can show the
//! on-screen mark; the clear sound it plays is a short tone through `ultar-audiod`.
//!
//! Enrolment never sends audio over the bus: the Settings page calls [`Enroll`], the service
//! records the next take itself and counts it in [`VoiceState`].

use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "voice";

/// Sample rate of every recording inside the service (Hertz).
pub const SAMPLE_RATE: u32 = 16_000;
/// Longest request recorded after the wake word (seconds).
pub const MAX_REQUEST_SECS: u16 = 15;
/// Takes needed before a word or the voice counts as enrolled.
pub const TAKES: u8 = 3;
/// Most samples one microphone chunk or one speech clip carries (10 s at [`SAMPLE_RATE`]).
pub const MAX_CHUNK_SAMPLES: usize = SAMPLE_RATE as usize * 10;

/// What the service is doing right now.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Phase {
    /// Off, or waiting without listening.
    #[default]
    Idle,
    /// Listening for the wake word (or the secret word while locked).
    Listening,
    /// The wake word was heard; recording the request.
    Recording,
    /// Turning the speech into text and asking the helper.
    Thinking,
    /// Speaking the answer aloud.
    Answering,
    /// Recording one enrolment take.
    Enrolling,
}

/// How the Settings test ("say Hey KM") stands.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TestResult {
    /// No test ran (or the last one was dismissed).
    #[default]
    None,
    /// Waiting for the person to say the word.
    Waiting,
    /// The word was heard.
    Heard,
    /// Nothing was heard in time.
    Missed,
}

/// Why voice control cannot work (or an enrolment take failed), for the Settings page.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum VoiceError {
    /// The audio service delivers no microphone stream.
    NoAudio,
    /// The speech-to-text package or its model is not installed.
    NoStt,
    /// The speech output package is not installed (answers stay silent).
    NoTts,
    /// Switched on, but the wake word was not recorded three times yet.
    NotEnrolled,
    /// The take was too short or too quiet.
    TakeShort,
    /// The take does not sound like the earlier ones.
    TakeDiffers,
    /// No take was heard in time.
    TakeMissed,
}

/// One speech-to-text model offered in Settings.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ModelInfo {
    /// Model name (data, e.g. `small`).
    pub name: String,
    /// Size on disk in megabytes.
    pub size_mb: u32,
    /// The model file is on the device (models come with the offline update).
    pub installed: bool,
}

/// How far the training of one thing is.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Training {
    /// It is trained (recorded [`TAKES`] times).
    pub enrolled: bool,
    /// Takes so far (0 to [`TAKES`]).
    pub clips: u8,
}

/// What the service has to work with.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Ready {
    /// The speech-to-text package and a model are installed.
    pub stt: bool,
    /// The speech output package is installed.
    pub tts: bool,
    /// The audio service delivers a microphone stream.
    pub mic: bool,
}

/// `voice.state` - what the person switched on and what the service is doing (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct VoiceState {
    /// Voice control is switched on.
    pub enabled: bool,
    /// What the pipeline is doing (see [`Phase`]).
    pub phase: Phase,
    /// Wake word in use (data; the standard "Hey KM").
    pub wake_word: String,
    /// The wake word (recorded [`TAKES`] times; the page shows "x of 3").
    pub wake: Training,
    /// The secret word; it also works while locked.
    pub secret: Training,
    /// The person's voice for the speaker check, over the enrolment sentences.
    pub voice: Training,
    /// Speech language tag (`ar`, `en`, `de`).
    pub language: String,
    /// Chosen speech-to-text model (data; empty: the first installed one).
    pub model: String,
    /// The speech-to-text models, with their sizes.
    pub models: Vec<ModelInfo>,
    /// What is installed and what the audio service delivers.
    pub ready: Ready,
    /// The screen is locked: only the secret word wakes it, and only harmless things run.
    pub locked: bool,
    /// The last request came from another voice; the person must confirm by fingerprint or face.
    pub verify_needed: bool,
    /// The Settings test.
    pub test: TestResult,
    /// Why the service cannot work right now, for the surface (`None`: fine).
    pub error: Option<VoiceError>,
}

/// What to enrol: the wake word, the secret word, or the person's voice.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnrollKind {
    /// The wake word, recorded [`TAKES`] times.
    Wake,
    /// The secret word (also while locked), recorded [`TAKES`] times.
    Secret,
    /// The person's voice for the speaker check, recorded over the given sentence.
    Voice,
}

/// `voice.set_enabled` - switch voice control on or off (off by default).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetEnabled {
    /// On.
    pub on: bool,
}

/// `voice.set_language` - the speech language.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SetLanguage {
    /// `ar`, `en` or `de`.
    pub tag: String,
}

/// `voice.set_model` - which speech-to-text model to use.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SetModel {
    /// Model name from [`VoiceState::models`].
    pub name: String,
}

/// `voice.enroll` - record the next take of the wake word, the secret word or the voice.
/// The service listens for the person's next utterance; the take counts in [`VoiceState`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Enroll {
    /// What is being enrolled.
    pub kind: EnrollKind,
}

/// `voice.forget` - delete the enrolment of one kind (all its takes).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Forget {
    /// What to forget.
    pub kind: EnrollKind,
}

/// `voice.test` - run the pipeline once ("say the wake word"), for the Settings test button.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Test {}

/// `voice.set_locked` - the lock screen tells the service whether the screen is locked.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetLocked {
    /// The screen is locked.
    pub locked: bool,
}

/// `voice.owner_confirmed` - the lock screen or the sign-in service tells that the person
/// confirmed by fingerprint or face after [`VoiceState::verify_needed`]; the next request is
/// then accepted without the speaker check (one time, a short while).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct OwnerConfirmed {}

/// How a change or action went.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum VoiceOutcome {
    /// Done.
    Done,
    /// Voice control is switched off (switch it on first).
    Disabled,
    /// Enrolment is not complete yet (the wake word was not recorded three times).
    NotEnrolled,
    /// No microphone stream or no speaker.
    NoAudio,
    /// Another request is being answered.
    Busy,
    /// Unknown language or model.
    Invalid,
    /// The model is not on the device.
    NotInstalled,
    /// Nothing to confirm.
    Nothing,
}

/// Answer to the changes.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct VoiceReply {
    /// The result.
    pub outcome: VoiceOutcome,
}

/// `audio.mic` - one chunk of the microphone (PCM16 mono), published by the audio service
/// only while somebody asked for it with [`Capture`] (not retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct MicChunk {
    /// Sample rate in Hertz (the service needs [`SAMPLE_RATE`]).
    pub rate: u32,
    /// Samples.
    pub samples: Vec<i16>,
}

/// `audio.capture` - ask the audio service to start or stop the microphone stream.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Capture {
    /// Start (true) or stop.
    pub on: bool,
}

/// `audio.play` - play a short clip (the answer, the listening tone) on the output in use.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Play {
    /// Sample rate in Hertz.
    pub rate: u32,
    /// Samples (PCM16 mono).
    pub samples: Vec<i16>,
}

/// Answer of the audio service to [`Capture`] and [`Play`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct AudioDone {
    /// It worked (false: no device).
    pub ok: bool,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    VoiceState,
    SetEnabled,
    SetLanguage,
    SetModel,
    Enroll,
    Forget,
    Test,
    SetLocked,
    OwnerConfirmed,
    VoiceReply,
    MicChunk,
    Capture,
    Play,
    AudioDone
);

impl Topic for VoiceState {
    const TOPIC: &'static str = "voice.state";
    const RETAINED: bool = true;
}

impl Topic for MicChunk {
    const TOPIC: &'static str = "audio.mic";
    const RETAINED: bool = false;
}

macro_rules! call {
    ($t:ty, $svc:expr, $m:literal, $r:ty) => {
        impl Call for $t {
            const SERVICE: &'static str = $svc;
            const METHOD: &'static str = $m;
            type Reply = $r;
        }
    };
}
call!(SetEnabled, SERVICE, "set_enabled", VoiceReply);
call!(SetLanguage, SERVICE, "set_language", VoiceReply);
call!(SetModel, SERVICE, "set_model", VoiceReply);
call!(Enroll, SERVICE, "enroll", VoiceReply);
call!(Forget, SERVICE, "forget", VoiceReply);
call!(Test, SERVICE, "test", VoiceReply);
call!(SetLocked, SERVICE, "set_locked", VoiceReply);
call!(OwnerConfirmed, SERVICE, "owner_confirmed", VoiceReply);
call!(Capture, "audio", "capture", AudioDone);
call!(Play, "audio", "play", AudioDone);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn messages_round_trip() {
        let s = VoiceState {
            enabled: true,
            phase: Phase::Listening,
            wake_word: "Hey KM".into(),
            wake: Training {
                enrolled: true,
                clips: 3,
            },
            voice: Training {
                enrolled: true,
                clips: 3,
            },
            language: "ar".into(),
            model: "small".into(),
            models: vec![ModelInfo {
                name: "small".into(),
                size_mb: 466,
                installed: true,
            }],
            ready: Ready {
                stt: true,
                tts: true,
                mic: true,
            },
            locked: true,
            verify_needed: false,
            test: TestResult::Heard,
            error: Some(VoiceError::NoAudio),
            ..VoiceState::default()
        };
        assert_eq!(decode::<VoiceState>(&encode(&s).unwrap()).unwrap(), s);

        let chunk = MicChunk {
            rate: SAMPLE_RATE,
            samples: vec![0, 1, -2, 32000],
        };
        assert_eq!(decode::<MicChunk>(&encode(&chunk).unwrap()).unwrap(), chunk);

        let r = VoiceReply {
            outcome: VoiceOutcome::NotEnrolled,
        };
        assert_eq!(decode::<VoiceReply>(&encode(&r).unwrap()).unwrap(), r);
    }

    #[test]
    fn services_and_methods_are_valid() {
        assert_eq!(SERVICE, "voice");
        assert_eq!(SetEnabled::METHOD, "set_enabled");
        assert_eq!(Enroll::METHOD, "enroll");
        assert_eq!(Test::METHOD, "test");
        assert_eq!(SetLocked::METHOD, "set_locked");
        assert_eq!(Capture::SERVICE, "audio");
        assert_eq!(Play::METHOD, "play");
        assert_eq!(MicChunk::TOPIC, "audio.mic");
    }
}
