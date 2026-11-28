//! Contract of the voice control service (`ultar-voiced`): the person says the wake word,
//! the helper hears the request and answers aloud. Everything stays on the device.
//!
//! Voice control is **off** until the person switches it on. The service owns no
//! microphone of its own: it records only through `ultar-audiod`, never before the wake
//! word, and stores nothing but the person's enrolment data (sealed in its state
//! directory). While it listens it publishes [`VoiceState`] with `listening: true` so the
//! shell can show the on-screen mark; the clear sound it plays is a short tone through
//! `ultar-audiod`.

use crate::message::{Call, Message, Text, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "voice";

/// Sample rate of every recording inside the service (Hertz).
pub const SAMPLE_RATE: u32 = 16_000;
/// Longest request recorded after the wake word (seconds).
pub const MAX_REQUEST_SECS: u16 = 15;
/// Most samples one enrolment clip may carry (a few seconds at [`SAMPLE_RATE`]).
pub const MAX_CLIP_SAMPLES: usize = SAMPLE_RATE as usize * 5;

/// What the service is doing right now.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Phase {
    /// Off or waiting without listening.
    #[default]
    Idle,
    /// Listening for the wake word (or the secret word while locked).
    Listening,
    /// The wake word was heard; recording the request.
    Recording,
    /// Turning the speech into text, asking the helper, and forming the answer.
    Thinking,
    /// Speaking the answer aloud.
    Answering,
}

/// `voice.state` – what the person switched on and what the service is doing (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct VoiceState {
    /// Voice control is switched on.
    pub enabled: bool,
    /// What the pipeline is doing (see [`Phase`]).
    pub phase: Phase,
    /// Wake word in use (data; the standard "Hey KM" until the person changes it).
    pub wake_word: String,
    /// The wake word was enrolled (recorded three times).
    pub wake_enrolled: bool,
    /// Wake-word recordings so far (0–3; the Settings page shows "x of 3").
    pub wake_clips: u8,
    /// A secret word was enrolled (recorded three times); it also works while locked.
    pub secret_enrolled: bool,
    /// Secret-word recordings so far (0–3).
    pub secret_clips: u8,
    /// The person's voice was enrolled for the speaker check.
    pub voice_enrolled: bool,
    /// Voice recordings so far over the enrolment sentence (0–3).
    pub voice_clips: u8,
    /// Choose STT language tag (`ar`, `en`, `de`).
    pub language: String,
    /// Chosen speech-to-text model (data; empty: default). Downloads happen in Settings.
    pub model: String,
    /// The screen is locked: only the secret word wakes it, and only harmless actions run.
    pub locked: bool,
    /// Why the service cannot work right now, for the surface (`None`: fine).
    pub error: Option<Text>,
}

/// What to enrol: the wake word, the secret word, or the person's voice.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnrollKind {
    /// The wake word, recorded three times.
    Wake,
    /// The secret word (also while locked), recorded three times.
    Secret,
    /// The person's voice for the speaker check, recorded over the given sentence.
    Voice,
}

/// One recording in linear PCM16 mono at [`SAMPLE_RATE`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Clip {
    /// Samples in the order they were recorded.
    pub samples: Vec<i16>,
}

/// `voice.set_enabled` – switch voice control on or off (off by default).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetEnabled {
    /// On.
    pub on: bool,
}

/// `voice.set_language` – the speech-to-text language.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SetLanguage {
    /// `ar`, `en` or `de`.
    pub tag: String,
}

/// `voice.set_model` – which speech-to-text model to use (downloaded in Settings).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SetModel {
    /// Model name from the Settings list.
    pub name: String,
}

/// `voice.enroll` – one recording of the wake word, the secret word, or the voice.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Enroll {
    /// What is being enrolled.
    pub kind: EnrollKind,
    /// The recording.
    pub clip: Clip,
}

/// `voice.test` – run the pipeline once ("say the wake word"), for the Settings test button.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Test {}

/// `voice.set_locked` – the lock screen tells the service it is locked now.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetLocked {
    /// The screen is locked.
    pub locked: bool,
}

/// How a change or action went.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum VoiceOutcome {
    /// Done.
    Done,
    /// Voice control is switched off (switch it on first).
    Disabled,
    /// Enrolment is not complete yet (e.g. the wake word was not recorded three times).
    NotEnrolled,
    /// No audio device (no microphone or speaker).
    NoAudio,
    /// Another request is being answered.
    Busy,
    /// The clip is empty or too large.
    Invalid,
    /// The voice or word was not recognised.
    NotRecognised,
    /// The helper or a backend refused or failed.
    Failed,
}

/// Answer to the changes.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct VoiceReply {
    /// The result.
    pub outcome: VoiceOutcome,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    VoiceState,
    Clip,
    SetEnabled,
    SetLanguage,
    SetModel,
    Enroll,
    Test,
    SetLocked,
    VoiceReply
);

impl Topic for VoiceState {
    const TOPIC: &'static str = "voice.state";
    const RETAINED: bool = true;
}

macro_rules! call {
    ($t:ty, $m:literal) => {
        impl Call for $t {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $m;
            type Reply = VoiceReply;
        }
    };
}
call!(SetEnabled, "set_enabled");
call!(SetLanguage, "set_language");
call!(SetModel, "set_model");
call!(Enroll, "enroll");
call!(Test, "test");
call!(SetLocked, "set_locked");

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
            wake_enrolled: true,
            wake_clips: 3,
            secret_enrolled: false,
            secret_clips: 0,
            voice_enrolled: true,
            voice_clips: 3,
            language: "ar".into(),
            model: "whisper-small".into(),
            locked: true,
            error: Some(Text::key("voice.error.no_audio")),
        };
        assert_eq!(decode::<VoiceState>(&encode(&s).unwrap()).unwrap(), s);

        let clip = Clip {
            samples: vec![0, 1, -2, 32000],
        };
        assert_eq!(decode::<Clip>(&encode(&clip).unwrap()).unwrap(), clip);

        let r = VoiceReply {
            outcome: VoiceOutcome::NotEnrolled,
        };
        assert_eq!(decode::<VoiceReply>(&encode(&r).unwrap()).unwrap(), r);
    }

    #[test]
    fn service_and_methods_are_valid() {
        assert_eq!(SERVICE, "voice");
        assert_eq!(SetEnabled::METHOD, "set_enabled");
        assert_eq!(Enroll::METHOD, "enroll");
        assert_eq!(Test::METHOD, "test");
        assert_eq!(SetLocked::METHOD, "set_locked");
    }
}