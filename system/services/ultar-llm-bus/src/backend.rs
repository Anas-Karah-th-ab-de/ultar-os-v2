//! Talking to the model service: three requests over its Unix socket (HTTP/1.1 with JSON,
//! the service's own interface).

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;
use ultar_bus::messages::llm::{LlmState, ModelClass, MAX_ANSWER};

/// Models up to this size count as small (about 2 billion parameters, 4-bit).
pub const SMALL_BYTES: u64 = 1_700_000_000;
/// Models up to this size count as medium (about 8 billion parameters, 4-bit).
pub const MEDIUM_BYTES: u64 = 5_800_000_000;
const LOOK_TIME: Duration = Duration::from_secs(3);
const LONGEST_REPLY: u64 = 4 * 1024 * 1024;

/// The class of a model file of `bytes`.
pub fn class_of(bytes: u64) -> ModelClass {
    if bytes <= SMALL_BYTES {
        ModelClass::Small
    } else if bytes <= MEDIUM_BYTES {
        ModelClass::Medium
    } else {
        ModelClass::Large
    }
}

/// The model service behind its socket.
#[derive(Clone, Debug)]
pub struct Backend {
    socket: PathBuf,
}

/// The body of an HTTP reply with status 200; `None` for anything else.
pub fn body_of(reply: &[u8]) -> Option<&[u8]> {
    let end = reply.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&reply[..end]).ok()?;
    let status = head.lines().next()?.split_whitespace().nth(1)?;
    (status == "200").then(|| &reply[end + 4..])
}

impl Backend {
    /// The model service at `socket`.
    pub fn new(socket: &Path) -> Self {
        Self {
            socket: socket.to_owned(),
        }
    }

    fn request(&self, method: &str, path: &str, body: &str, wait: Duration) -> Option<Value> {
        let mut stream = UnixStream::connect(&self.socket).ok()?;
        stream.set_read_timeout(Some(wait)).ok()?;
        stream.set_write_timeout(Some(LOOK_TIME)).ok()?;
        let head = format!(
            "{method} {path} HTTP/1.1\r\nHost: llm\r\nConnection: close\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).ok()?;
        stream.write_all(body.as_bytes()).ok()?;
        let mut reply = Vec::new();
        stream.take(LONGEST_REPLY).read_to_end(&mut reply).ok()?;
        serde_json::from_slice(body_of(&reply)?).ok()
    }

    /// What the model service can do now: ready when it answers and a model is installed;
    /// the class is that of the model in use, or of the largest installed one.
    pub fn state(&self) -> LlmState {
        let health = self.request("GET", "/health", "", LOOK_TIME);
        let models = self.request("GET", "/v1/models", "", LOOK_TIME);
        let (Some(health), Some(models)) = (health, models) else {
            return LlmState::default();
        };
        let list = models["data"].as_array().cloned().unwrap_or_default();
        let size = |m: &Value| m["meta"]["bytes"].as_u64().unwrap_or(0);
        let loaded = health["model"].as_str();
        let chosen = list
            .iter()
            .find(|m| m["id"].as_str() == loaded && loaded.is_some())
            .or_else(|| list.iter().max_by_key(|m| size(m)));
        match chosen {
            Some(model) => LlmState {
                ready: true,
                model: model["id"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(96)
                    .collect(),
                class: class_of(size(model)),
            },
            None => LlmState::default(),
        }
    }

    /// Lets the model answer; `None` when it did not.
    pub fn complete(
        &self,
        system: &str,
        prompt: &str,
        max_tokens: u32,
        wait: Duration,
    ) -> Option<String> {
        let body = json!({
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": prompt},
            ],
            "max_tokens": max_tokens,
            "stream": false,
        });
        let reply = self.request("POST", "/v1/chat/completions", &body.to_string(), wait)?;
        let text = reply["choices"][0]["message"]["content"].as_str()?;
        let mut end = text.len().min(MAX_ANSWER);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        (end > 0).then(|| text[..end].to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_sizes_become_classes() {
        assert_eq!(class_of(400_000_000), ModelClass::Small);
        assert_eq!(class_of(1_100_000_000), ModelClass::Small);
        assert_eq!(class_of(2_500_000_000), ModelClass::Medium);
        assert_eq!(class_of(5_000_000_000), ModelClass::Medium);
        assert_eq!(class_of(20_000_000_000), ModelClass::Large);
    }

    #[test]
    fn only_a_good_reply_has_a_body() {
        assert_eq!(
            body_of(b"HTTP/1.1 200 OK\r\nA: b\r\n\r\n{}"),
            Some(&b"{}"[..])
        );
        assert_eq!(body_of(b"HTTP/1.1 503 Busy\r\n\r\n{}"), None);
        assert_eq!(body_of(b"garbage"), None);
    }

    #[test]
    fn a_missing_model_service_means_no_model() {
        let nowhere = Backend::new(Path::new("/nonexistent/llm.sock"));
        assert_eq!(nowhere.state(), LlmState::default());
        assert_eq!(nowhere.complete("s", "p", 8, LOOK_TIME), None);
    }
}
