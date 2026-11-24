//! Talking to a model: the rules it gets, and reading what it answers. The model never
//! acts itself – it names a tool of the list, the helper decides whether and how to use it.

use crate::tools::{Call, TOOLS};
use serde::Deserialize;
use std::collections::BTreeMap;

/// Longest answer of the model in tokens.
pub const MAX_TOKENS: u32 = 256;

/// Something that lets a model answer.
pub trait Thinker {
    /// The model's answer to `prompt` under `system`; `None` when no model answered.
    fn think(&self, system: &str, prompt: &str) -> Option<String>;
}

/// What a model decided.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Move {
    /// Use a tool.
    Use(Call),
    /// Say this to the person and stop.
    Say(String),
}

/// The rules for the model: who it is, the tools, and the one form its answers have.
pub fn system_prompt() -> String {
    let mut text = String::from(
        "You are the helper of this device. You answer in the language of the person, \
         Arabic first. You can only act through these tools:\n",
    );
    for tool in TOOLS {
        text.push_str(&format!(
            "- {}({}): {}\n",
            tool.name,
            tool.args.join(", "),
            tool.about
        ));
    }
    text.push_str(
        "Answer with exactly one JSON object and nothing else: \
         {\"tool\":\"name\",\"args\":{\"key\":\"value\"}} to use a tool, or \
         {\"say\":\"text\"} to answer the person. After a tool you are told its result. \
         Never invent a tool. If you cannot help, say so.",
    );
    text
}

#[derive(Deserialize)]
struct Raw {
    tool: Option<String>,
    #[serde(default)]
    args: BTreeMap<String, serde_json::Value>,
    say: Option<String>,
}

fn plain(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// What the model's answer means; `None` when it kept to no form.
pub fn read(answer: &str) -> Option<Move> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    let raw: Raw = serde_json::from_str(answer.get(start..=end)?).ok()?;
    if let Some(tool) = raw.tool.filter(|t| !t.is_empty()) {
        let args = raw
            .args
            .iter()
            .map(|(k, v)| (k.clone(), plain(v)))
            .collect();
        return Some(Move::Use(Call { tool, args }));
    }
    raw.say.filter(|s| !s.trim().is_empty()).map(Move::Say)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_is_told_every_tool_and_the_form_of_its_answers() {
        let rules = system_prompt();
        for tool in TOOLS {
            assert!(rules.contains(tool.name), "{}", tool.name);
        }
        assert!(rules.contains("{\"say\":\"text\"}"));
    }

    #[test]
    fn answers_are_read_even_with_talk_around_them() {
        let used = read("Sure!\n{\"tool\":\"set_volume\",\"args\":{\"percent\":40}}\n").unwrap();
        assert_eq!(
            used,
            Move::Use(Call::new("set_volume", &[("percent", "40")]))
        );
        let on = read("{\"tool\":\"wifi\",\"args\":{\"on\":true}}").unwrap();
        assert_eq!(on, Move::Use(Call::new("wifi", &[("on", "true")])));
        assert_eq!(read("{\"say\":\"done\"}"), Some(Move::Say("done".into())));
    }

    #[test]
    fn what_keeps_to_no_form_is_not_guessed() {
        for bad in [
            "",
            "ok",
            "{\"say\":\"  \"}",
            "{\"tool\":\"\"}",
            "{broken",
            "}{",
        ] {
            assert_eq!(read(bad), None, "{bad}");
        }
    }
}
