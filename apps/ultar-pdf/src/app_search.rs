//! Search in the reader: the bar, the query, the hits and moving between them.

use crate::app::App;
use crate::doc::DocState;
use crate::search::{self, Hit};
use crate::state::rect_to_px;
use ultar_ui::ui::UiError;

impl App {
    /// Opens the search bar (Ctrl+F).
    pub fn open_search(&mut self) -> Result<(), UiError> {
        if self.doc.state != DocState::Open {
            return Ok(());
        }
        self.find.open = true;
        self.rebuild()
    }

    /// Closes the search bar and forgets the hits.
    pub fn close_search(&mut self) -> Result<(), UiError> {
        self.find.open = false;
        self.find.hits.clear();
        self.doc.search(self.find.seq, Vec::new(), 0);
        self.rebuild()
    }

    /// Searches `query` (a blank query clears the hits).
    pub fn start_search(&mut self, query: &str) -> Result<(), UiError> {
        self.find.seq = self.find.seq.wrapping_add(1);
        let tokens = search::tokens(query);
        self.find
            .hits
            .begin(query, self.find.seq, self.doc.sizes.len());
        self.doc.search(self.find.seq, tokens, self.view.page);
        self.sync_chrome()?;
        self.draw()
    }

    pub(crate) fn found(&mut self, id: u32, hits: Vec<Hit>) -> Result<(), UiError> {
        let first = self.find.hits.add_page(id, hits);
        self.sync_chrome()?;
        if first {
            self.show_hit()?;
        }
        Ok(())
    }

    /// Moves to the next (`dir > 0`) or previous hit and scrolls to it.
    pub fn step_hit(&mut self, dir: i8) -> Result<(), UiError> {
        let found = if dir >= 0 {
            self.find.hits.next().is_some()
        } else {
            self.find.hits.prev().is_some()
        };
        if found {
            self.show_hit()?;
        }
        Ok(())
    }

    /// Scrolls so that the shown hit is in view (a third from the top).
    pub(crate) fn show_hit(&mut self) -> Result<(), UiError> {
        let Some(hit) = self.find.hits.current().cloned() else {
            return Ok(());
        };
        let (vw, vh) = self.view_px();
        let (Some(row), Some(size), Some(r)) = (
            self.view.layout.rows.get(hit.page).copied(),
            self.doc.sizes.get(hit.page).copied(),
            hit.rects.first().copied(),
        ) else {
            return Ok(());
        };
        let (l, t, rr, b) = rect_to_px(r, size, self.view.scale, self.view.rot);
        self.view.scroll = row.y + (t + b) / 2.0 - vh / 3.0;
        let centre = (self.view.layout.content_w.max(vw) - row.w) / 2.0 + (l + rr) / 2.0;
        self.view.scroll_x = if self.view.layout.content_w > vw {
            centre - vw / 2.0
        } else {
            0.0
        };
        self.view.page = hit.page;
        self.after_move(false)
    }
}
