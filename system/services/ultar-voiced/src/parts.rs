//! Small parts of the service's state: the microphone stream, the Settings test and the
//! problems the person is told about.

use std::time::Instant;
use ultar_bus::messages::voice::{TestResult, VoiceError};

/// The microphone stream: asked for, delivering.
#[derive(Default)]
pub(crate) struct Mic {
    /// The audio service was told to deliver (chunks that nobody asked for are dropped).
    pub(crate) on: bool,
    pub(crate) asked: Option<Instant>,
    /// Chunks arrive (or the audio service said yes).
    pub(crate) ready: bool,
    pub(crate) last_chunk: Option<Instant>,
}

/// The Settings test of the wake word.
#[derive(Default)]
pub(crate) struct Trial {
    pub(crate) result: TestResult,
    pub(crate) until: Option<Instant>,
}

/// What the person must be told: the last take and the last request went wrong.
#[derive(Default)]
pub(crate) struct Errors {
    pub(crate) take: Option<VoiceError>,
    pub(crate) job: Option<VoiceError>,
}
