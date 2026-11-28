//! Everything the app can be asked to do besides the person's keys: the window's `Out`
//! results, and the assistant's actions (`docs/APP-RULES.md` section 9). The assistant may
//! open a document and read it freely; inserting or replacing text changes the document and
//! therefore waits for the person's confirmation in a dialog. Every action is visible in the
//! status line, and an edit is an ordinary undo step.

use crate::app::App;
use crate::dialogs::Ask;
use crate::state::Doc;
use std::path::Path;
use ultar_ui::ui::UiError;
use ultar_ui::widgets::Label;

/// Longest text an action may carry (bytes).
pub const MAX_TEXT: usize = 1 << 20;

/// What the window has to do for the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    /// Offer this text on the clipboard.
    Copy(String),
    /// Ask for the clipboard text (it comes back through `App::pasted`).
    Paste,
    /// Show this title.
    Title(String),
    /// The window may close now.
    Close,
}

/// An agent action. `open` and `read_text` are free, `insert_text` and `replace` confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentAction {
    Open { uri: String },
    ReadText { max: usize },
    InsertText { text: String },
    Replace { find: String, repl: String },
}

impl AgentAction {
    /// The bus/agent name (as in the manifest).
    pub fn name(&self) -> &'static str {
        match self {
            AgentAction::Open { .. } => "open",
            AgentAction::ReadText { .. } => "read_text",
            AgentAction::InsertText { .. } => "insert_text",
            AgentAction::Replace { .. } => "replace",
        }
    }

    /// Whether the person must confirm before it runs.
    pub fn confirm(&self) -> bool {
        matches!(
            self,
            AgentAction::InsertText { .. } | AgentAction::Replace { .. }
        )
    }
}

/// The answer to the assistant: plain text it can read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentResult(pub String);

/// Opens the `files://` URI and returns the document, or the key of the error.
pub fn open_doc(uri: &str, areas: &Path) -> Result<Doc, &'static str> {
    let path = crate::place::path_of(uri, areas).ok_or("doc.reason.name")?;
    Doc::open(&path).map_err(|e| e.key())
}

/// The document as text, cut at `max` characters (the assistant's `read_text`).
pub fn read_text(doc: &Doc, max: usize) -> String {
    let t = doc.document.to_text();
    if t.chars().count() > max {
        t.chars().take(max).collect::<String>() + "\n..."
    } else {
        t
    }
}

impl App {
    /// An action of the assistant. Free actions answer at once; the others open a
    /// confirmation and answer later through [`App::take_agent_results`] (`None` now).
    pub fn agent(&mut self, action: AgentAction) -> Result<Option<AgentResult>, UiError> {
        let too_big = match &action {
            AgentAction::InsertText { text } => text.len() > MAX_TEXT,
            AgentAction::Replace { find, repl } => {
                find.len() > MAX_TEXT || repl.len() > MAX_TEXT || find.is_empty()
            }
            _ => false,
        };
        if too_big {
            return Ok(Some(AgentResult("error: text empty or too large".into())));
        }
        match action {
            AgentAction::ReadText { max } => {
                Ok(Some(AgentResult(read_text(&self.doc, max.min(MAX_TEXT)))))
            }
            AgentAction::Open { uri } => self.agent_open(&uri).map(Some),
            a if self.ask.is_some() => {
                let _ = a;
                Ok(Some(AgentResult("busy: a question is open".into())))
            }
            a => {
                self.ask_for(Ask::Agent(a))?;
                Ok(None)
            }
        }
    }

    fn agent_open(&mut self, uri: &str) -> Result<AgentResult, UiError> {
        if self.doc.dirty() {
            return Ok(AgentResult(
                "refused: the open document has unsaved changes".into(),
            ));
        }
        match open_doc(uri, &self.areas) {
            Ok(doc) => {
                let name = doc.name().unwrap_or_default();
                self.install(doc, Some(uri.to_owned()))?;
                self.message = Some(Label::key_with("doc.agent.opened", &[("name", &name)]));
                self.status()?;
                Ok(AgentResult(format!("ok: opened {name}")))
            }
            Err(key) => Ok(AgentResult(format!("error: {key}"))),
        }
    }

    /// Takes the answers of confirmed (or declined) actions.
    pub fn take_agent_results(&mut self) -> Vec<AgentResult> {
        std::mem::take(&mut self.io.agent_results)
    }

    pub(crate) fn agent_declined(&mut self, a: &AgentAction) {
        self.io
            .agent_results
            .push(AgentResult(format!("declined: {}", a.name())));
        self.message = Some(Label::key("doc.agent.declined"));
    }

    /// Runs a confirmed action.
    pub(crate) fn agent_run(&mut self, a: &AgentAction) -> Result<(), UiError> {
        self.doc.burst_break();
        let (text, label) = match a {
            AgentAction::InsertText { text } => {
                self.doc.insert(text);
                (
                    format!("ok: inserted {} characters", text.chars().count()),
                    Label::key("doc.agent.inserted"),
                )
            }
            AgentAction::Replace { find, repl } => {
                let n = self.doc.replace_everywhere(find, repl, false);
                (
                    format!("ok: replaced {n}"),
                    Label::key_with("doc.agent.replaced", &[("n", &n.to_string())]),
                )
            }
            _ => return Ok(()),
        };
        self.doc.burst_break();
        self.io.agent_results.push(AgentResult(text));
        self.message = Some(label);
        self.changed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_flags() {
        assert!(!AgentAction::Open { uri: String::new() }.confirm());
        assert!(!AgentAction::ReadText { max: 10 }.confirm());
        assert!(AgentAction::InsertText {
            text: String::new()
        }
        .confirm());
        assert!(AgentAction::Replace {
            find: String::new(),
            repl: String::new()
        }
        .confirm());
    }

    #[test]
    fn read_text_is_cut() {
        let mut d = Doc::new();
        d.insert("abcdef");
        assert_eq!(read_text(&d, 100), "abcdef\n");
        assert!(read_text(&d, 3).starts_with("abc\n"));
    }

    #[test]
    fn the_names_match_the_manifest() {
        let manifest = include_str!("../ultar-app.toml");
        for a in [
            AgentAction::Open { uri: String::new() },
            AgentAction::ReadText { max: 1 },
            AgentAction::InsertText {
                text: String::new(),
            },
            AgentAction::Replace {
                find: String::new(),
                repl: String::new(),
            },
        ] {
            assert!(
                manifest.contains(&format!("\"{}\"", a.name())),
                "{} is in the manifest",
                a.name()
            );
        }
    }
}
