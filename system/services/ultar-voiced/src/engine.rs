//! The device backends behind the pipeline: microphone, speech-to-text and text-to-speech.
//!
//! The real ones are the `whisper` and `espeak` packages built by `ultar-paketbau`, started
//! only when they are needed. Until those packages are installed the service runs with the
//! [`Silent`] stand-ins, which report "missing" and let everything else keep working; the
//! pipeline and the locked-screen rules stay fully testable with the in-memory fakes used
//! by the tests.

/// Why a backend could not do its work (goes to the log and to [`VoiceOutcome`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The package is not installed (the Settings page offers the download).
    Missing(&'static str),
    /// It ran but failed.
    Failed(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Missing(package) => write!(f, "package {package} not installed"),
            EngineError::Failed(why) => f.write_str(why),
        }
    }
}

/// Turns a recording into text.
pub trait Transcriber: Send + Sync {
    /// Transcribes PCM16 mono.
    fn transcribe(
        &self,
        pcm16: &[i16],
        sample_rate: u32,
        language: &str,
    ) -> Result<String, EngineError>;
}

/// Speaks a text aloud.
pub trait Speaker: Send + Sync {
    /// Synthesises and plays `text`.
    fn say(&self, text: &str, language: &str) -> Result<(), EngineError>;
}

/// The microphone stream (capture comes from `ultar-audiod`).
pub trait Listener: Send + Sync {
    /// The next PCM16 chunk, or `None` when nothing new arrived.
    fn next_chunk(&self) -> Result<Option<Vec<i16>>, EngineError>;
}

/// Stand-ins that say "not installed" instead of pretending to hear or speak.
#[derive(Default)]
pub struct Silent;

impl Transcriber for Silent {
    fn transcribe(
        &self,
        _pcm16: &[i16],
        _sample_rate: u32,
        _language: &str,
    ) -> Result<String, EngineError> {
        Err(EngineError::Missing("whisper"))
    }
}

impl Speaker for Silent {
    fn say(&self, _text: &str, _language: &str) -> Result<(), EngineError> {
        Err(EngineError::Missing("espeak"))
    }
}

impl Listener for Silent {
    fn next_chunk(&self) -> Result<Option<Vec<i16>>, EngineError> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_backends_report_missing() {
        let t = Silent;
        let s = Silent;
        let l = Silent;
        assert_eq!(
            t.transcribe(&[0i16; 16], 16_000, "ar"),
            Err(EngineError::Missing("whisper"))
        );
        assert_eq!(s.say("x", "ar"), Err(EngineError::Missing("espeak")));
        assert_eq!(l.next_chunk(), Ok(None));
    }

    #[test]
    fn errors_have_a_text() {
        assert_eq!(
            EngineError::Missing("whisper").to_string(),
            "package whisper not installed"
        );
        assert_eq!(EngineError::Failed("x".into()).to_string(), "x");
    }
}