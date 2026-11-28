//! Everything the app can be asked to do: the window's `Out` results, and the agent's
//! actions (`docs/APP-RULES.md` §9). The agent may open and read freely; inserting or
//! replacing text needs a confirmation from the person.

use crate::state::Doc;
use std::path::Path;

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
    /// The bus/agent name (used by the manifest and the announcement).
    pub fn name(&self) -> &'static str {
        match self {
            AgentAction::Open { .. } => "open",
            AgentAction::ReadText { .. } => "read_text",
            AgentAction::InsertText { .. } => "insert_text",
            AgentAction::Replace { .. } => "replace",
        }
    }

    /// Whether the action must be confirmed by the person before it runs.
    pub fn confirm(&self) -> bool {
        matches!(
            self,
            AgentAction::InsertText { .. } | AgentAction::Replace { .. }
        )
    }
}

/// The result of an agent action (text, so the agent reads it back).
pub struct AgentResult(pub String);

/// Opens the URI of a `files://` document and returns the document and any open error.
pub fn open_doc(uri: &str, areas: &Path) -> (Doc, Result<String, String>) {
    let Some(path) = crate::place::path_of(uri, areas) else {
        return (Doc::new(), Err("bad uri".into()));
    };
    match Doc::open(&path) {
        Ok(d) => (d, Ok(path_to_string(&path))),
        Err(e) => (Doc::new(), Err(e.key().into())),
    }
}

/// The whole document as text (for `read_text`).
pub fn read_text(doc: &Doc, max: usize) -> String {
    let t = doc.document.to_text();
    if t.chars().count() > max {
        t.chars().take(max).collect::<String>() + "\n…"
    } else {
        t
    }
}

/// The agent's insert: at the caret, after the person confirmed.
pub fn insert_text(doc: &mut Doc, text: &str) {
    doc.insert(text);
}

/// The agent's replace: replaces every occurrence and reports the count.
pub fn replace(doc: &mut Doc, find: &str, repl: &str) -> usize {
    doc.burst_break();
    let n = doc.document.replace(find, repl);
    if n > 0 {
        doc.dirty = true;
    }
    n
}

fn path_to_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_insert_and_replace_confirm() {
        let mut d = Doc::new();
        insert_text(&mut d, "hello");
        assert_eq!(read_text(&d, 100), "hello\n");
        assert_eq!(replace(&mut d, "hello", "مرحبا"), 1);
        assert!(read_text(&d, 100).contains("مرحبا"));
    }

    #[test]
    fn confirmation_flags() {
        assert!(!AgentAction::Open { uri: String::new() }.confirm());
        assert!(!AgentAction::ReadText { max: 10 }.confirm());
        assert!(AgentAction::InsertText { text: String::new() }.confirm());
        assert!(AgentAction::Replace { find: String::new(), repl: String::new() }.confirm());
    }
}