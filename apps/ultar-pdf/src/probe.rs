//! What the outside may look at without touching the reader's insides: where the parts of the
//! window are, where the search stands. Used by the window tests and the helper's read-back.

use crate::app::App;
use crate::state::rect_to_px;
use ultar_ui::geom::{Point, Rect};

impl App {
    /// The page area in logical pixels (after layout).
    pub fn page_area(&mut self) -> Option<Rect> {
        self.ui.layout();
        self.nodes.page.and_then(|n| self.ui.rect(n))
    }

    /// The thumbnail or outline area of the start-side panel (wide windows only).
    pub fn panel_area(&mut self) -> Option<Rect> {
        self.ui.layout();
        let node = self.nodes.side.thumbs.or(self.nodes.side.outline)?;
        self.ui.rect(node)
    }

    /// The search field (while the search bar is open).
    pub fn search_area(&mut self) -> Option<Rect> {
        self.ui.layout();
        self.nodes.finder.search_field.and_then(|n| self.ui.rect(n))
    }

    /// The shown hit as (position from 1, total), when there is one.
    pub fn hit_position(&self) -> Option<(usize, usize)> {
        Some((self.find.hits.position()? + 1, self.find.hits.count()))
    }

    /// The whole document has been searched.
    pub fn search_done(&self) -> bool {
        self.find.hits.done()
    }

    /// The search bar is open.
    pub fn search_open(&self) -> bool {
        self.find.open
    }

    /// The text selected on its page, as `(page, first word, last word)`.
    pub fn selection_range(&self) -> Option<(usize, usize, usize)> {
        self.pick.selection.map(|s| (s.page, s.first, s.last))
    }

    /// The middle of word `word` of `page` in window pixels, when its text is loaded and its
    /// page has a place in the layout (it may be scrolled out of view).
    pub fn word_point(&mut self, page: usize, word: usize) -> Option<Point> {
        let text = self.doc.text(page)?;
        let w = text.words.get(word)?;
        let size = self.doc.sizes.get(page).copied()?;
        let row = self.view.layout.rows.get(page).copied()?;
        let (l, t, r, b) = rect_to_px(
            (w.x0, w.y0, w.x1, w.y1),
            size,
            self.view.scale,
            self.view.rot,
        );
        let (vw, _) = self.view_px();
        let x0 = (self.view.layout.content_w.max(vw) - row.w) / 2.0 - self.view.scroll_x;
        let s = self.ui.env().scale;
        let area = self.page_area()?;
        Some(Point {
            x: area.x + (x0 + (l + r) / 2.0) / s,
            y: area.y + (row.y - self.view.scroll + (t + b) / 2.0) / s,
        })
    }

    /// The page number field gets the keyboard (as a click into it would).
    pub fn focus_page_field(&mut self) -> Result<(), ultar_ui::ui::UiError> {
        match self.nodes.bar.page_field {
            Some(f) => self.ui.focus(f),
            None => Ok(()),
        }
    }

    /// An open question (the open dialog, the password dialog) is on screen.
    pub fn asking(&self) -> bool {
        self.ask.is_some()
    }
}
