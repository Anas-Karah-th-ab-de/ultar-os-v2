//! The device backends behind the service: speech to text and text to speech.
//!
//! The real ones are the `whisper` and `espeak-ng` packages built by `ultar-paketbau`
//! ([`crate::whisper`], [`crate::espeak`]); their processes are started only when they are
//! needed. Without those packages the service runs with the [`Silent`] stand-ins, which say
//! "not installed" instead of pretending to hear or speak, and everything else keeps working.
//! The pipeline and the lock rules stay fully testable with the stand-ins of the tests.

use ultar_bus::messages::voice::Play;

/// Why a backend could not do its work (goes to the log and decides what the person sees).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The package (or the model) is not installed.
    Missing(&'static str),
    /// It ran but failed.
    Failed(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Missing(what) => write!(f, "{what} not installed"),
            EngineError::Failed(why) => f.write_str(why),
        }
    }
}

/// Turns a recording into text.
pub trait Transcriber: Send + Sync {
    /// The program and a model are on the device.
    fn installed(&self, model: &str) -> bool;
    /// Transcribes PCM16 mono in `language` (`ar`, `en`, `de`) with `model` (empty: the
    /// first installed one). The text is empty when nothing was understood.
    fn transcribe(
        &self,
        pcm16: &[i16],
        rate: u32,
        language: &str,
        model: &str,
    ) -> Result<String, EngineError>;
    /// Called every few seconds: stops what has been idle for long.
    fn idle_check(&self) {}
}

/// Speaks a text aloud.
pub trait Speaker: Send + Sync {
    /// The program is on the device.
    fn installed(&self) -> bool;
    /// Synthesises `text` in `language` as a clip for the audio service.
    fn synthesize(&self, text: &str, language: &str) -> Result<Play, EngineError>;
}

/// Stand-ins that say "not installed".
#[derive(Default)]
pub struct Silent;

impl Transcriber for Silent {
    fn installed(&self, _model: &str) -> bool {
        false
    }

    fn transcribe(
        &self,
        _pcm16: &[i16],
        _rate: u32,
        _language: &str,
        _model: &str,
    ) -> Result<String, EngineError> {
        Err(EngineError::Missing("whisper"))
    }
}

impl Speaker for Silent {
    fn installed(&self) -> bool {
        false
    }

    fn synthesize(&self, _text: &str, _language: &str) -> Result<Play, EngineError> {
        Err(EngineError::Missing("espeak-ng"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_backends_report_missing() {
        assert!(!Transcriber::installed(&Silent, ""));
        assert!(!Speaker::installed(&Silent));
        assert_eq!(
            Silent.transcribe(&[0i16; 16], 16_000, "ar", ""),
            Err(EngineError::Missing("whisper"))
        );
        assert_eq!(
            Silent.synthesize("x", "ar"),
            Err(EngineError::Missing("espeak-ng"))
        );
    }

    #[test]
    fn errors_have_a_text() {
        assert_eq!(
            EngineError::Missing("whisper").to_string(),
            "whisper not installed"
        );
        assert_eq!(EngineError::Failed("x".into()).to_string(), "x");
    }
}
