//! The providers behind the service: Claude through its Messages interface, and any
//! server that speaks the widely used chat-completions interface (the person's own
//! server, or another provider). Another interface is another [`Provider`].

use serde_json::{json, Value};
use std::time::Duration;
use ultar_bus::messages::llm::MAX_ANSWER;
use ultar_bus::messages::llm_cloud::{ModelInfo, MAX_ENDPOINT, MAX_MODELS};

/// Why the provider gave no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The provider refused the key.
    Key,
    /// No connection, a time-out, or an answer that is none.
    Unreachable,
}

/// Where and what to ask.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Target {
    /// Address without a path (`https://…`).
    pub endpoint: String,
    /// Model id.
    pub model: String,
}

/// A provider of a language model.
pub trait Provider: Send + Sync {
    /// Stable id.
    fn id(&self) -> &'static str;
    /// Its name, as the person reads it.
    fn name(&self) -> &'static str;
    /// Its fixed address; `None`: the person gives one.
    fn endpoint(&self) -> Option<&str>;
    /// A word in the id of its small, fast models (the default choice).
    fn small(&self) -> &'static str;
    /// The models the key may use.
    fn models(&self, at: &Target, key: &str) -> Result<Vec<ModelInfo>, Failure>;
    /// One answer.
    fn complete(&self, at: &Target, key: &str, ask: &Ask<'_>) -> Result<String, Failure>;
}

/// One request to a model.
#[derive(Debug, Clone, Copy)]
pub struct Ask<'a> {
    /// The rules.
    pub system: &'a str,
    /// The request.
    pub prompt: &'a str,
    /// Longest answer in tokens.
    pub tokens: u32,
}

/// Longest wait for a list of models.
const LIST_WAIT: Duration = Duration::from_secs(15);
/// Longest wait for one answer.
const ANSWER_WAIT: Duration = Duration::from_secs(100);

/// An address a key may be sent to: `https://host…` without spaces or a user part; plain
/// `http` only towards this device itself.
pub fn endpoint_ok(endpoint: &str) -> bool {
    let local = ["http://127.0.0.1", "http://localhost", "http://[::1]"];
    let after = |prefix: &str| endpoint.strip_prefix(prefix).map(str::to_owned);
    let host = after("https://").or_else(|| {
        let own = local.iter().find(|l| {
            let rest = endpoint.strip_prefix(**l);
            rest.is_some_and(|r| r.is_empty() || r.starts_with([':', '/']))
        });
        own.and_then(|_| after("http://"))
    });
    let plain = |c: char| c.is_ascii_graphic() && c != '@' && c != '\\';
    host.is_some_and(|h| !h.is_empty() && !h.starts_with('/') && h.chars().all(plain))
        && endpoint.len() <= MAX_ENDPOINT
}

fn cut(mut text: String) -> Option<String> {
    let mut end = text.len().min(MAX_ANSWER);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (!text.trim().is_empty()).then_some(text)
}

fn failure(error: &ureq::Error) -> Failure {
    match error {
        ureq::Error::Status(401 | 403, _) => Failure::Key,
        _ => Failure::Unreachable,
    }
}

fn url(at: &Target, path: &str) -> String {
    format!("{}{path}", at.endpoint.trim_end_matches('/'))
}

/// The models of a list answer (`data[].id`, with `display_name` when given).
pub fn models_of(reply: &Value) -> Vec<ModelInfo> {
    let list = reply["data"].as_array().cloned().unwrap_or_default();
    let model = |m: &Value| {
        let id = m["id"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 128)?;
        let name = m["display_name"].as_str().unwrap_or(id);
        Some(ModelInfo {
            id: id.to_owned(),
            name: name.chars().take(64).collect(),
        })
    };
    list.iter().filter_map(model).take(MAX_MODELS).collect()
}

/// The model to ask: the wanted one when the provider offers it, else a small one, else
/// the first; without a list, the wanted one.
pub fn pick_model(models: &[ModelInfo], wanted: &str, small: &str) -> String {
    let offered = |id: &str| models.iter().any(|m| m.id == id);
    if models.is_empty() || offered(wanted) {
        return wanted.to_owned();
    }
    let small = models.iter().find(|m| m.id.contains(small));
    small
        .or(models.first())
        .map_or_else(String::new, |m| m.id.clone())
}

/// Claude's address.
pub const CLAUDE_ENDPOINT: &str = "https://api.anthropic.com";
/// The version of Claude's interface this service speaks.
pub const CLAUDE_API_VERSION: &str = "2026-10-21";

/// Claude (`POST /v1/messages`, `GET /v1/models`).
#[derive(Debug, Clone)]
pub struct Claude {
    /// Address without a path.
    pub endpoint: String,
    /// Interface version (`anthropic-version`).
    pub version: String,
}

impl Default for Claude {
    fn default() -> Self {
        Self {
            endpoint: CLAUDE_ENDPOINT.to_owned(),
            version: CLAUDE_API_VERSION.to_owned(),
        }
    }
}

/// The text of a Claude answer: all text blocks in order.
pub fn claude_text(reply: &Value) -> Option<String> {
    let blocks = reply["content"].as_array()?;
    let texts = blocks.iter().filter(|b| b["type"] == "text");
    cut(texts.filter_map(|b| b["text"].as_str()).collect())
}

impl Claude {
    fn request(&self, method: &str, url: &str, key: &str, wait: Duration) -> ureq::Request {
        ureq::request(method, url)
            .timeout(wait)
            .set("authorization", &format!("Bearer {key}"))
            .set("anthropic-version", &self.version)
            .set("content-type", "application/json")
    }
}

impl Provider for Claude {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn name(&self) -> &'static str {
        "Claude"
    }

    fn endpoint(&self) -> Option<&str> {
        Some(&self.endpoint)
    }

    fn small(&self) -> &'static str {
        "haiku"
    }

    fn models(&self, at: &Target, key: &str) -> Result<Vec<ModelInfo>, Failure> {
        let list = url(at, "/v1/models?limit=100");
        let sent = self.request("GET", &list, key, LIST_WAIT).call();
        let reply: Value = sent
            .map_err(|e| failure(&e))?
            .into_json()
            .map_err(|_| Failure::Unreachable)?;
        Ok(models_of(&reply))
    }

    fn complete(&self, at: &Target, key: &str, ask: &Ask<'_>) -> Result<String, Failure> {
        let body = json!({
            "model": at.model,
            "max_tokens": ask.tokens,
            "system": ask.system,
            "messages": [{"role": "user", "content": ask.prompt}],
        });
        let sent = self
            .request("POST", &url(at, "/v1/messages"), key, ANSWER_WAIT)
            .send_json(body);
        let reply: Value = sent
            .map_err(|e| failure(&e))?
            .into_json()
            .map_err(|_| Failure::Unreachable)?;
        claude_text(&reply).ok_or(Failure::Unreachable)
    }
}

/// A server with the chat-completions interface (`POST /v1/chat/completions`,
/// `GET /v1/models`) at an address the person gives.
#[derive(Debug, Clone, Copy, Default)]
pub struct Compatible;

/// The text of a chat-completions answer.
pub fn compatible_text(reply: &Value) -> Option<String> {
    let text = reply["choices"][0]["message"]["content"].as_str()?;
    cut(text.to_owned())
}

impl Provider for Compatible {
    fn id(&self) -> &'static str {
        "compatible"
    }

    fn name(&self) -> &'static str {
        "OpenAI-compatible"
    }

    fn endpoint(&self) -> Option<&str> {
        None
    }

    fn small(&self) -> &'static str {
        "mini"
    }

    fn models(&self, at: &Target, key: &str) -> Result<Vec<ModelInfo>, Failure> {
        let sent = ureq::get(&url(at, "/v1/models"))
            .timeout(LIST_WAIT)
            .set("authorization", &format!("Bearer {key}"))
            .call();
        let reply: Value = sent
            .map_err(|e| failure(&e))?
            .into_json()
            .map_err(|_| Failure::Unreachable)?;
        Ok(models_of(&reply))
    }

    fn complete(&self, at: &Target, key: &str, ask: &Ask<'_>) -> Result<String, Failure> {
        let body = json!({
            "model": at.model,
            "max_tokens": ask.tokens,
            "stream": false,
            "messages": [
                {"role": "system", "content": ask.system},
                {"role": "user", "content": ask.prompt},
            ],
        });
        let sent = ureq::post(&url(at, "/v1/chat/completions"))
            .timeout(ANSWER_WAIT)
            .set("authorization", &format!("Bearer {key}"))
            .send_json(body);
        let reply: Value = sent
            .map_err(|e| failure(&e))?
            .into_json()
            .map_err(|_| Failure::Unreachable)?;
        compatible_text(&reply).ok_or(Failure::Unreachable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_is_the_text_in_order() {
        let reply = json!({"type":"message","content":[
            {"type":"text","text":"{\"say\":"},
            {"type":"tool_use","id":"x"},
            {"type":"text","text":"\"hello\"}"}]});
        assert_eq!(claude_text(&reply).as_deref(), Some("{\"say\":\"hello\"}"));
        assert_eq!(claude_text(&json!({"content":[]})), None);
        assert_eq!(
            claude_text(&json!({"type":"error","error":{"type":"x"}})),
            None
        );
        let long = json!({"content":[{"type":"text","text":"é".repeat(MAX_ANSWER)}]});
        assert!(claude_text(&long).unwrap().len() <= MAX_ANSWER);
        let chat = json!({"choices":[{"message":{"role":"assistant","content":"hi"}}]});
        assert_eq!(compatible_text(&chat).as_deref(), Some("hi"));
        assert_eq!(compatible_text(&json!({"choices":[]})), None);
    }

    #[test]
    fn a_key_goes_only_to_an_encrypted_address_or_to_this_device() {
        for good in [
            "https://api.anthropic.com",
            "https://llm.example.org:8443/base",
            "http://127.0.0.1:8080",
            "http://localhost",
        ] {
            assert!(endpoint_ok(good), "{good}");
        }
        for bad in [
            "http://example.org",
            "http://127.0.0.1.evil.org",
            "http://localhost.evil.org",
            "https://",
            "https:///path",
            "https://user@example.org",
            "https://exa mple.org",
            "ftp://example.org",
            "example.org",
            "",
        ] {
            assert!(!endpoint_ok(bad), "{bad}");
        }
        assert!(!endpoint_ok(&format!(
            "https://{}",
            "a".repeat(MAX_ENDPOINT)
        )));
    }

    #[test]
    fn the_model_is_the_wanted_one_or_a_small_one_the_provider_offers() {
        let model = |id: &str| ModelInfo {
            id: id.into(),
            name: id.into(),
        };
        let list = json!({"data":[
            {"type":"model","id":"claude-opus-5-5","display_name":"Claude Opus 5.5"},
            {"id":"claude-haiku-5-5"}, {"id":""}, {"name":"no id"}]});
        let models = models_of(&list);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "Claude Opus 5.5");
        assert_eq!(models[1].name, "claude-haiku-5-5");
        assert_eq!(
            pick_model(&models, "claude-opus-5-5", "haiku"),
            "claude-opus-5-5"
        );
        assert_eq!(
            pick_model(&models, "gone-model", "haiku"),
            "claude-haiku-5-5"
        );
        assert_eq!(pick_model(&models, "", "haiku"), "claude-haiku-5-5");
        assert_eq!(pick_model(&[model("only")], "gone", "haiku"), "only");
        // no list (the provider was not reached): what was wanted stays
        assert_eq!(pick_model(&[], "wanted", "haiku"), "wanted");
    }
}
