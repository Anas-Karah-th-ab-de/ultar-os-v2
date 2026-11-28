//! Text to speech with the `espeak-ng` package: one process per answer, the text on its
//! standard input, the speech as a WAV file on its standard output. The clip is handed to the
//! audio service; nothing is written to disk.

use crate::audio::decode_wav;
use crate::engine::{EngineError, Speaker};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;
use ultar_bus::messages::voice::{Play, MAX_CHUNK_SAMPLES};

/// How long one answer may take to synthesise.
pub const TIMEOUT: Duration = Duration::from_secs(30);
/// Longest text spoken at once (bytes); the rest is cut at a character boundary.
pub const MAX_TEXT: usize = 2_000;

/// The eSpeak NG voice of a language tag.
pub fn voice_of(language: &str) -> Option<&'static str> {
    match language {
        "ar" => Some("ar"),
        "en" => Some("en-us"),
        "de" => Some("de"),
        _ => None,
    }
}

/// The speech output of the `espeak-ng` package.
pub struct EspeakSpeaker {
    bin: PathBuf,
    data: Option<PathBuf>,
}

impl EspeakSpeaker {
    /// The program `bin`; `data` is the directory that holds `espeak-ng-data` (`None`: the
    /// one compiled into the program).
    pub fn new(bin: PathBuf, data: Option<PathBuf>) -> Self {
        Self { bin, data }
    }
}

/// The text cut to [`MAX_TEXT`] bytes at a character boundary.
pub fn limited(text: &str) -> &str {
    if text.len() <= MAX_TEXT {
        return text;
    }
    let cut = (0..=MAX_TEXT)
        .rev()
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(0);
    &text[..cut]
}

impl Speaker for EspeakSpeaker {
    fn installed(&self) -> bool {
        self.bin.is_file()
    }

    fn synthesize(&self, text: &str, language: &str) -> Result<Play, EngineError> {
        if !self.bin.is_file() {
            return Err(EngineError::Missing("espeak-ng"));
        }
        let voice = voice_of(language).ok_or_else(|| EngineError::Failed("language".into()))?;
        let mut command = Command::new(&self.bin);
        command.args(["-v", voice, "--stdout", "--stdin"]);
        if let Some(data) = &self.data {
            command.arg(format!("--path={}", data.display()));
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| EngineError::Failed(format!("start: {e}")))?;
        let text = limited(text).to_owned();
        if let Some(mut stdin) = child.stdin.take() {
            // the process reads all of it before it speaks; a write error shows below
            let _ = stdin.write_all(text.as_bytes());
        }
        let bytes = match read_output(&mut child) {
            Ok(bytes) => bytes,
            Err(why) => {
                // too slow or broken: end it so it does not stay behind
                let _ = child.kill();
                let _ = child.wait();
                return Err(EngineError::Failed(format!("speech output {why}")));
            }
        };
        let status = child
            .wait()
            .map_err(|e| EngineError::Failed(format!("run: {e}")))?;
        if !status.success() {
            return Err(EngineError::Failed(format!("ended: {status}")));
        }
        let wav = decode_wav(&bytes).ok_or_else(|| EngineError::Failed("no WAV".into()))?;
        if wav.samples.is_empty() || wav.samples.len() > MAX_CHUNK_SAMPLES * 6 {
            return Err(EngineError::Failed("clip size".into()));
        }
        Ok(Play {
            rate: wav.rate,
            samples: wav.samples,
        })
    }
}

/// Everything the child writes to stdout, or why that failed within the time limit.
fn read_output(child: &mut std::process::Child) -> Result<Vec<u8>, &'static str> {
    let Some(mut stdout) = child.stdout.take() else {
        return Err("missing");
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = stdout.read_to_end(&mut bytes).map(|_| bytes);
        let _ = tx.send(read);
    });
    match rx.recv_timeout(TIMEOUT) {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(_)) => Err("read failed"),
        Err(_) => Err("timed out"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_map_to_voices() {
        assert_eq!(voice_of("ar"), Some("ar"));
        assert_eq!(voice_of("en"), Some("en-us"));
        assert_eq!(voice_of("de"), Some("de"));
        assert_eq!(voice_of("../x"), None);
    }

    #[test]
    fn long_text_is_cut_at_a_character() {
        let text = "ع".repeat(MAX_TEXT);
        let cut = limited(&text);
        assert!(cut.len() <= MAX_TEXT && !cut.is_empty());
        assert!(text.starts_with(cut));
        assert_eq!(limited("short"), "short");
    }

    fn program(name: &str, body: &str) -> EspeakSpeaker {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("voiced-espeak-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("espeak-ng");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        EspeakSpeaker::new(path, None)
    }

    #[test]
    fn a_program_that_fails_or_writes_no_speech_is_reported() {
        let failing = program("fails", "cat > /dev/null; exit 3");
        assert!(matches!(
            failing.synthesize("x", "en"),
            Err(EngineError::Failed(why)) if why.starts_with("ended")
        ));
        let garbage = program("garbage", "cat > /dev/null; echo not a wav");
        assert_eq!(
            garbage.synthesize("x", "en"),
            Err(EngineError::Failed("no WAV".into()))
        );
    }

    #[test]
    fn without_the_program_it_says_missing() {
        let speaker = EspeakSpeaker::new(PathBuf::from("/nonexistent/espeak-ng"), None);
        assert!(!speaker.installed());
        assert_eq!(
            speaker.synthesize("x", "ar"),
            Err(EngineError::Missing("espeak-ng"))
        );
    }
}
