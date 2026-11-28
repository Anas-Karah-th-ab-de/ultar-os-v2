//! The slow part of one spoken request, run on a helper thread of the service: speech to text,
//! the helper's answer, speech output, playing. It never touches the service's state; it returns
//! how it ended.

use crate::engine::{EngineError, Speaker, Transcriber};
use crate::speech;
use std::time::Duration;
use ultar_bus::messages::agent::AskVoice;
use ultar_bus::messages::voice::{VoiceError, SAMPLE_RATE};
use ultar_bus::{Client, Text};

/// How long the helper may take to answer (a local model thinks for a while).
pub const ASK_TIME: Duration = Duration::from_secs(120);
/// How long playing one clip may take (the audio service answers when it is played).
pub const PLAY_TIME: Duration = Duration::from_secs(90);

/// What a job does.
pub enum Work {
    /// Transcribe the request, ask the helper, speak the answer.
    Request {
        /// The request (PCM16 mono, 16 kHz).
        pcm: Vec<i16>,
        /// The screen was locked when it was spoken.
        locked: bool,
    },
    /// Only say these sentences of the service (`voice.say.*`).
    Say(Vec<&'static str>),
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct End {
    /// Something is not installed or failed in a way the person should see.
    pub error: Option<VoiceError>,
}

/// What a job needs besides the work.
pub struct Setup<'a> {
    /// Speech language tag.
    pub language: &'a str,
    /// Chosen model (empty: the best installed).
    pub model: &'a str,
    /// The bus client.
    pub client: &'a Client,
    /// Speech to text.
    pub stt: &'a dyn Transcriber,
    /// Text to speech.
    pub tts: &'a dyn Speaker,
}

fn speak(setup: &Setup<'_>, words: &str, end: &mut End) {
    if words.trim().is_empty() {
        return;
    }
    match setup.tts.synthesize(words, setup.language) {
        Ok(clip) => {
            let _ = setup.client.call(&clip, PLAY_TIME);
        }
        Err(EngineError::Missing(_)) => end.error = Some(VoiceError::NoTts),
        Err(EngineError::Failed(_)) => {}
    }
}

fn say_key(setup: &Setup<'_>, key: &str, end: &mut End) {
    let tr = speech::translator(setup.language);
    speak(setup, &speech::say(&tr, &Text::key(key)), end);
}

/// Runs the work; `answering` is told when the helper's answer is about to be spoken.
pub fn run(setup: &Setup<'_>, work: Work, answering: &dyn Fn()) -> End {
    let mut end = End::default();
    match work {
        Work::Say(keys) => {
            for key in keys {
                say_key(setup, key, &mut end);
            }
        }
        Work::Request { pcm, locked } => {
            let text = match setup
                .stt
                .transcribe(&pcm, SAMPLE_RATE, setup.language, setup.model)
            {
                Ok(text) => text,
                Err(EngineError::Missing(_)) => {
                    end.error = Some(VoiceError::NoStt);
                    return end;
                }
                Err(EngineError::Failed(_)) => String::new(),
            };
            if text.trim().is_empty() {
                say_key(setup, "voice.say.not_understood", &mut end);
                return end;
            }
            match setup.client.call(&AskVoice { text, locked }, ASK_TIME) {
                Ok(answer) => {
                    answering();
                    let tr = speech::translator(setup.language);
                    speak(setup, &speech::say(&tr, &answer.text), &mut end);
                }
                Err(_) => say_key(setup, "voice.say.helper_failed", &mut end),
            }
        }
    }
    end
}
