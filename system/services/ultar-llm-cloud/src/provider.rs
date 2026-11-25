//! The provider behind the service. Today one: Claude, through its Messages interface
//! (`POST <endpoint>/v1/messages`). Another provider is another [`Provider`].

use serde_json::{json, Value};
use std::time::Duration;
use ultar_bus::messages::llm::MAX_ANSWER;

/// Why the provider gave no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The provider refused the key.
    Key,
    /// No connection, a time-out, or an answer that is none.
    Unreachable,
}

/// A provider of a language model.
pub trait Provider: Send + Sync {
    /// Its name, as the person reads it.
    fn name(&self) -> &str;
    /// The model asked for.
    fn model(&self) -> &str;
    /// One answer.
    fn complete(
        &self,
        key: &str,
        system: &str,
        prompt: &str,
        tokens: u32,
    ) -> Result<String, Failure>;
}

/// The provider's address unless another is given.
pub const CLAUDE_ENDPOINT: &str = "https://api.anthropic.com";
/// The model asked for unless another is given (the small, fast one).
pub const CLAUDE_MODEL: &str = "claude-haiku-5-5";
/// The version of the interface this service speaks.
pub const CLAUDE_API_VERSION: &str = "2026-10-21";
/// The provider's name.
pub const CLAUDE_NAME: &str = "Claude";

/// Claude.
#[derive(Debug, Clone)]
pub struct Claude {
    /// Address without a path (`https://…`).
    pub endpoint: String,
    /// Model id.
    pub model: String,
    /// Interface version (`anthropic-version`).
    pub version: String,
    /// Longest wait for one answer.
    pub wait: Duration,
}

impl Default for Claude {
    fn default() -> Self {
        Self {
            endpoint: CLAUDE_ENDPOINT.to_owned(),
            model: CLAUDE_MODEL.to_owned(),
            version: CLAUDE_API_VERSION.to_owned(),
            wait: Duration::from_secs(100),
        }
    }
}

/// The text of an answer: all text blocks in order, cut to the bus limit.
pub fn text_of(reply: &Value) -> Option<String> {
    let blocks = reply["content"].as_array()?;
    let texts = blocks.iter().filter(|b| b["type"] == "text");
    let mut text: String = texts.filter_map(|b| b["text"].as_str()).collect();
    let mut end = text.len().min(MAX_ANSWER);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (!text.trim().is_empty()).then_some(text)
}

impl Provider for Claude {
    fn name(&self) -> &str {
        CLAUDE_NAME
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn complete(
        &self,
        key: &str,
        system: &str,
        prompt: &str,
        tokens: u32,
    ) -> Result<String, Failure> {
        let body = json!({
            "model": self.model,
            "max_tokens": tokens,
            "system": system,
            "messages": [{"role": "user", "content": prompt}],
        });
        let url = format!("{}/v1/messages", self.endpoint.trim_end_matches('/'));
        let sent = ureq::post(&url)
            .timeout(self.wait)
            .set("x-api-key", key)
            .set("anthropic-version", &self.version)
            .set("content-type", "application/json")
            .send_json(body);
        match sent {
            Ok(response) => {
                let reply: Value = response.into_json().map_err(|_| Failure::Unreachable)?;
                text_of(&reply).ok_or(Failure::Unreachable)
            }
            Err(ureq::Error::Status(401 | 403, _)) => Err(Failure::Key),
            Err(_) => Err(Failure::Unreachable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_is_the_text_blocks_in_order() {
        let reply = json!({"type":"message","content":[
            {"type":"text","text":"{\"say\":"},
            {"type":"tool_use","id":"x"},
            {"type":"text","text":"\"hello\"}"}]});
        assert_eq!(text_of(&reply).as_deref(), Some("{\"say\":\"hello\"}"));
        assert_eq!(text_of(&json!({"content":[]})), None);
        assert_eq!(text_of(&json!({"type":"error","error":{"type":"x"}})), None);
        let long = json!({"content":[{"type":"text","text":"é".repeat(MAX_ANSWER)}]});
        assert!(text_of(&long).unwrap().len() <= MAX_ANSWER);
    }
}
