//! What the helper (the agent) may do with the reader, APP-RULES §9. All four are free
//! (open, look, search, copy): they only show or read what the person could do by hand.
//! The manifest lists the same names under `[agent]`. Each action shows a notice, so the
//! person sees that the helper did it.

use crate::app::{App, Out};
use crate::doc::DocState;
use crate::place;
use ultar_ui::ui::UiError;
use ultar_ui::widgets::{Label, Toast, Tone};

/// The names of the actions, as the manifest lists them.
pub const NAMES: [&str; 4] = ["open", "go_to_page", "search", "copy_text"];

/// One thing the helper asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentAction {
    /// Open a document (`files://<area>/<path>[#page=N]`).
    Open { uri: String },
    /// Go to a page (from 1).
    GoToPage { page: usize },
    /// Search the document and show the hits.
    Search { query: String },
    /// Copy the text of a page (from 1), the current page, or the selection when `page` is
    /// `None` and something is selected.
    CopyText { page: Option<usize> },
}

/// What came of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentReply {
    /// Done (a search and a copy of a page not yet read finish a moment later).
    Done,
    /// No document is open.
    NotOpen,
    /// The page does not exist.
    OutOfRange { pages: usize },
    /// The address names no file the reader may open, or the query is blank.
    Invalid,
}

impl App {
    /// Carries out one action of the helper.
    pub fn agent(&mut self, action: AgentAction) -> Result<AgentReply, UiError> {
        let open = self.doc.state == DocState::Open;
        let pages = self.doc.sizes.len();
        let (notice, reply) = match action {
            AgentAction::Open { uri } => {
                if place::path_of(&uri, &self.areas).is_none() {
                    return Ok(AgentReply::Invalid);
                }
                self.open_entry(&uri)?;
                ("pdf.agent.open", AgentReply::Done)
            }
            _ if !open => return Ok(AgentReply::NotOpen),
            AgentAction::GoToPage { page } => {
                if page == 0 || page > pages {
                    return Ok(AgentReply::OutOfRange { pages });
                }
                self.goto(page - 1)?;
                ("pdf.agent.go_to_page", AgentReply::Done)
            }
            AgentAction::Search { query } => {
                if crate::search::tokens(&query).is_empty() {
                    return Ok(AgentReply::Invalid);
                }
                self.find.open = true;
                self.find.hits.query = query.clone();
                self.rebuild()?;
                self.start_search(&query)?;
                ("pdf.agent.search", AgentReply::Done)
            }
            AgentAction::CopyText { page } => {
                if page.is_some_and(|p| p == 0 || p > pages) {
                    return Ok(AgentReply::OutOfRange { pages });
                }
                self.copy_page_text(page)?;
                ("pdf.agent.copy_text", AgentReply::Done)
            }
        };
        let toast = Toast::new(1, Tone::Info, Label::key(notice));
        self.ui.show_toast(toast)?;
        Ok(reply)
    }

    /// Copies a page's text (or the selection); asks the worker when the text is not here yet.
    fn copy_page_text(&mut self, page: Option<usize>) -> Result<(), UiError> {
        if page.is_none() && self.selected_text().is_some() {
            self.copy_selection();
            return Ok(());
        }
        let index = page.map_or(self.view.page, |p| p - 1);
        match self.doc.text(index) {
            Some(text) => self.out.push(Out::Copy(text.all())),
            None => {
                self.pick.copy_page = Some(index);
                self.update_wants();
            }
        }
        Ok(())
    }
}
