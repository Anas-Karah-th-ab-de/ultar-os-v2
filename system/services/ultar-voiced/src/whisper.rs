//! Speech to text with the `whisper` package: `whisper-server` is started on the loopback
//! address when a spoken request has to be transcribed, answers one HTTP request per
//! recording, and is stopped again when it has been idle. Its output is not logged (it would
//! contain what the person said).

use crate::audio::encode_wav;
use crate::engine::{EngineError, Transcriber};
use crate::http::{can_connect, post_form, Form};
use crate::models::ModelCatalog;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long the server may take to load a model and listen.
pub const START_TIMEOUT: Duration = Duration::from_secs(60);
/// How long one transcription may take.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// The server is stopped after this long without a request.
pub const IDLE_STOP: Duration = Duration::from_secs(120);
/// The path the server answers inference requests on.
const INFERENCE_PATH: &str = "/inference";

struct Running {
    child: Child,
    model: String,
    last_used: Instant,
}

impl Running {
    fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The speech-to-text server of the `whisper` package, started on demand.
pub struct WhisperServer {
    bin: PathBuf,
    models: ModelCatalog,
    port: u16,
    threads: u32,
    running: Mutex<Option<Running>>,
}

impl WhisperServer {
    /// A server program `bin`, models from `models`, listening on 127.0.0.1:`port`.
    pub fn new(bin: PathBuf, models: ModelCatalog, port: u16, threads: u32) -> Self {
        Self {
            bin,
            models,
            port,
            threads,
            running: Mutex::new(None),
        }
    }

    fn addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.port))
    }

    /// Whether a server process is running now.
    pub fn is_running(&self) -> bool {
        self.running.lock().map(|r| r.is_some()).unwrap_or(false)
    }

    fn start(&self, model: &str, language: &str) -> Result<Running, EngineError> {
        let file = self
            .models
            .file(model)
            .ok_or(EngineError::Missing("model"))?;
        let child = Command::new(&self.bin)
            .arg("-m")
            .arg(&file)
            .args(["-l", language, "-nt", "--host", "127.0.0.1", "--port"])
            .arg(self.port.to_string())
            .args(["-t", &self.threads.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| EngineError::Failed(format!("start: {e}")))?;
        let mut running = Running {
            child,
            model: model.to_owned(),
            last_used: Instant::now(),
        };
        let end = Instant::now() + START_TIMEOUT;
        while Instant::now() < end {
            if can_connect(&self.addr(), Duration::from_millis(200)) {
                return Ok(running);
            }
            if let Ok(Some(status)) = running.child.try_wait() {
                return Err(EngineError::Failed(format!("server ended: {status}")));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        running.stop();
        Err(EngineError::Failed("server did not come up".into()))
    }
}

/// The `text` of the server's JSON answer, spaces trimmed.
pub fn text_of(body: &[u8]) -> Result<String, EngineError> {
    let value: serde_json::Value = serde_json::from_slice(body)
        .map_err(|_| EngineError::Failed("answer is not JSON".into()))?;
    value
        .get("text")
        .and_then(serde_json::Value::as_str)
        .map(|t| t.trim().to_owned())
        .ok_or_else(|| EngineError::Failed("answer has no text".into()))
}

impl Transcriber for WhisperServer {
    fn installed(&self, model: &str) -> bool {
        self.bin.is_file() && self.models.resolve(model).is_some()
    }

    fn transcribe(
        &self,
        pcm16: &[i16],
        rate: u32,
        language: &str,
        model: &str,
    ) -> Result<String, EngineError> {
        if !self.bin.is_file() {
            return Err(EngineError::Missing("whisper"));
        }
        let model = self
            .models
            .resolve(model)
            .ok_or(EngineError::Missing("model"))?;
        let mut slot = self
            .running
            .lock()
            .map_err(|_| EngineError::Failed("lock".into()))?;
        // another model, or a server that died: start afresh
        let dead = slot
            .as_mut()
            .is_some_and(|r| r.model != model || matches!(r.child.try_wait(), Ok(Some(_))));
        if dead {
            if let Some(old) = slot.take() {
                old.stop();
            }
        }
        if slot.is_none() {
            *slot = Some(self.start(&model, language)?);
        }
        let wav = encode_wav(rate, pcm16);
        let form = Form {
            fields: &[
                ("language", language),
                ("response_format", "json"),
                ("temperature", "0.0"),
            ],
            file_field: "file",
            file_name: "request.wav",
            file: &wav,
        };
        let answer = post_form(&self.addr(), INFERENCE_PATH, &form, REQUEST_TIMEOUT);
        if let Some(r) = slot.as_mut() {
            r.last_used = Instant::now();
        }
        match answer {
            Ok((200, body)) => text_of(&body),
            Ok((status, _)) => Err(EngineError::Failed(format!("server answered {status}"))),
            Err(e) => {
                // a server that does not answer is not kept
                if let Some(old) = slot.take() {
                    old.stop();
                }
                Err(EngineError::Failed(format!("request: {e}")))
            }
        }
    }

    fn idle_check(&self) {
        // a transcription in progress holds the lock: then it is not idle
        let Ok(mut slot) = self.running.try_lock() else {
            return;
        };
        let idle = slot
            .as_ref()
            .is_some_and(|r| r.last_used.elapsed() >= IDLE_STOP);
        if idle {
            if let Some(old) = slot.take() {
                old.stop();
            }
        }
    }
}

impl Drop for WhisperServer {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.running.lock() {
            if let Some(old) = slot.take() {
                old.stop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_of_an_answer_is_read() {
        let arabic = r#"{"text": " افتح الإعدادات\n"}"#;
        assert_eq!(text_of(arabic.as_bytes()).unwrap(), "افتح الإعدادات");
        assert!(text_of(b"not json").is_err());
        assert!(text_of(br#"{"nothing": 1}"#).is_err());
    }

    #[test]
    fn without_the_program_or_a_model_it_says_missing() {
        let dir = std::env::temp_dir().join(format!("voiced-whisper-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let server = WhisperServer::new(
            dir.join("whisper-server"),
            ModelCatalog::new(&dir),
            38_080,
            1,
        );
        assert!(!server.installed(""));
        assert_eq!(
            server.transcribe(&[0; 160], 16_000, "ar", ""),
            Err(EngineError::Missing("whisper"))
        );
        std::fs::write(dir.join("whisper-server"), b"x").unwrap();
        assert_eq!(
            server.transcribe(&[0; 160], 16_000, "ar", ""),
            Err(EngineError::Missing("model"))
        );
        assert!(!server.is_running());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
