//! Keys, pointer and touch on the reader: the shortcuts, wheel and drag scrolling, selecting
//! text, pinch zoom, and the clicks of the widgets.

use crate::app::{App, Ask, Drag, Out, PanelMode, Selection};
use crate::keys::{self, Context, Effect};
use crate::layout::Located;
use crate::state::{pinch_percent, px_to_page, Fit};
use crate::touch::Gesture;
use ultar_i18n::numbers::to_western_digits;
use ultar_ui::event::{Action, ActionKind, Event, Key, Modifiers, PointerButton};
use ultar_ui::geom::Point;
use ultar_ui::ui::{NodeId, UiError};

/// One line of scrolling in logical points.
const LINE: f32 = 40.0;
/// How far a press must move before it is a selection and not a click (logical points).
const SLOP: f32 = 4.0;

impl App {
    /// What the keyboard is doing now (for the key map and the window's shortcut filter).
    pub fn key_context(&self) -> Context {
        let focus = self.ui.focused();
        let typing = focus.is_some()
            && (focus == self.nodes.search_field
                || focus == self.nodes.page_field
                || focus == self.nodes.open_field
                || focus == self.nodes.password_field);
        Context {
            searching: self.searching,
            typing,
        }
    }

    /// A key the widgets did not use (or the window handed over before them).
    pub fn key(&mut self, key: Key, modifiers: Modifiers) -> Result<(), UiError> {
        if self.ask.is_some() {
            return Ok(());
        }
        let effect = keys::apply(key, modifiers, self.key_context());
        self.effect(effect)
    }

    /// Does what a shortcut names.
    pub fn effect(&mut self, effect: Effect) -> Result<(), UiError> {
        let open = self.doc.state == crate::doc::DocState::Open;
        let line = LINE * self.ui.env().scale;
        let (_, vh) = self.view_px();
        match effect {
            Effect::Open => {
                self.ask = Some(Ask::Open);
                return self.rebuild();
            }
            _ if !open => {}
            Effect::None => {}
            Effect::ZoomIn => self.zoom_by(1)?,
            Effect::ZoomOut => self.zoom_by(-1)?,
            Effect::ZoomReset => self.set_fit(Fit::Zoom(100))?,
            Effect::FitWidth => self.set_fit(Fit::Width)?,
            Effect::FitPage => self.set_fit(Fit::Page)?,
            Effect::Rotate => self.rotate()?,
            Effect::Search => self.open_search()?,
            Effect::CloseSearch => self.close_search()?,
            Effect::NextHit => self.step_hit(1)?,
            Effect::PrevHit => self.step_hit(-1)?,
            Effect::Copy => self.copy_selection(),
            Effect::SelectAll => self.select_all()?,
            Effect::ClearSelection => self.clear_selection()?,
            Effect::LineDown => self.scroll_by(0.0, line)?,
            Effect::LineUp => self.scroll_by(0.0, -line)?,
            Effect::LineLeft => self.scroll_by(-line, 0.0)?,
            Effect::LineRight => self.scroll_by(line, 0.0)?,
            Effect::PageDown => self.scroll_by(0.0, vh * 0.9)?,
            Effect::PageUp => self.scroll_by(0.0, -vh * 0.9)?,
            Effect::First => self.goto(0)?,
            Effect::Last => self.goto(self.doc.sizes.len().saturating_sub(1))?,
        }
        Ok(())
    }

    // ---- select and copy ------------------------------------------------------------------

    /// Offers the selected text on the clipboard (Ctrl+C).
    pub fn copy_selection(&mut self) {
        if let Some(text) = self.selected_text().filter(|t| !t.is_empty()) {
            self.out.push(Out::Copy(text));
        }
    }

    fn select_all(&mut self) -> Result<(), UiError> {
        let Some(text) = self.doc.text(self.page).filter(|t| !t.is_empty()) else {
            return Ok(());
        };
        self.selection = Some(Selection {
            page: self.page,
            first: 0,
            last: text.words.len() - 1,
        });
        self.draw()
    }

    fn clear_selection(&mut self) -> Result<(), UiError> {
        if self.selection.take().is_some() {
            self.draw()?;
        }
        Ok(())
    }

    // ---- pointer --------------------------------------------------------------------------

    /// A pointer event of the window (press, release, wheel, move with a button held).
    pub fn pointer(&mut self, event: Event) -> Result<(), UiError> {
        if self.ask.is_some() || self.doc.state != crate::doc::DocState::Open {
            return Ok(());
        }
        match event {
            Event::Scroll { pos, lines } => self.on_wheel(pos, lines),
            Event::PointerDown {
                pos,
                button: PointerButton::Primary,
            } => self.on_down(pos),
            Event::PointerMove { pos } => self.on_move(pos),
            Event::PointerUp {
                button: PointerButton::Primary,
                ..
            } => self.on_up(),
            _ => Ok(()),
        }
    }

    /// `pos` (window points) in the device pixels of `node`'s picture.
    fn in_node(&mut self, node: Option<NodeId>, pos: Point) -> Option<(f32, f32)> {
        self.ui.layout();
        let r = self.ui.rect(node?)?;
        let s = self.ui.env().scale;
        Some(((pos.x - r.x) * s, (pos.y - r.y) * s))
    }

    fn inside(&mut self, node: Option<NodeId>, pos: Point) -> bool {
        self.ui.layout();
        node.and_then(|n| self.ui.rect(n)).is_some_and(|r| r.contains(pos))
    }

    fn on_wheel(&mut self, pos: Point, lines: f32) -> Result<(), UiError> {
        let step = lines * LINE * self.ui.env().scale;
        if self.inside(self.nodes.page, pos) {
            return self.scroll_by(0.0, step);
        }
        if self.inside(self.nodes.thumbs, pos) {
            let h = self.view_h_of(self.nodes.thumbs);
            self.panel_scroll = (self.panel_scroll + step).clamp(0.0, self.thumbs.max_scroll(h));
            self.update_wants();
            return self.draw();
        }
        Ok(())
    }

    fn view_h_of(&mut self, node: Option<NodeId>) -> f32 {
        self.ui.layout();
        let s = self.ui.env().scale;
        node.and_then(|n| self.ui.rect(n)).map_or(0.0, |r| r.h * s)
    }

    fn on_down(&mut self, pos: Point) -> Result<(), UiError> {
        if self.inside(self.nodes.thumbs, pos) {
            return self.click_thumb(pos);
        }
        if !self.inside(self.nodes.page, pos) {
            return Ok(());
        }
        let had = self.selection.take().is_some();
        let (x, y) = self.in_node(self.nodes.page, pos).unwrap_or((0.0, 0.0));
        self.drag = self.word_at(x, y).map(|(page, word)| Drag {
            page,
            anchor: word,
            start: (pos.x, pos.y),
            moved: false,
        });
        if had {
            self.draw()?;
        }
        Ok(())
    }

    fn click_thumb(&mut self, pos: Point) -> Result<(), UiError> {
        let Some((x, y)) = self.in_node(self.nodes.thumbs, pos) else {
            return Ok(());
        };
        let w = self.thumbs.content_w;
        if let Some(Located { page, .. }) = self.thumbs.locate(x, y, w, self.panel_scroll, 0.0) {
            return self.goto(page);
        }
        Ok(())
    }

    /// The page and the word under (or nearest to) a point of the page area (device pixels).
    fn word_at(&mut self, x: f32, y: f32) -> Option<(usize, usize)> {
        let (vw, _) = self.view_px();
        let at = self.layout.locate(x, y, vw, self.scroll, self.scroll_x)?;
        let size = self.doc.sizes.get(at.page).copied()?;
        let text = self.doc.text(at.page)?;
        let p = px_to_page((at.x, at.y), size, self.scale, self.rot);
        text.nearest_word(p).map(|w| (at.page, w))
    }

    fn on_move(&mut self, pos: Point) -> Result<(), UiError> {
        let Some(mut drag) = self.drag else {
            return Ok(());
        };
        let s = self.ui.env().scale;
        let moved = drag.moved
            || (pos.x - drag.start.0).hypot(pos.y - drag.start.1) * s > SLOP * s;
        drag.moved = moved;
        self.drag = Some(drag);
        if !moved {
            return Ok(());
        }
        let (x, y) = self.in_node(self.nodes.page, pos).unwrap_or((0.0, 0.0));
        let Some(word) = self.drag_word(drag.page, x, y) else {
            return Ok(());
        };
        self.selection = Some(Selection {
            page: drag.page,
            first: drag.anchor.min(word),
            last: drag.anchor.max(word),
        });
        self.draw()
    }

    /// The word the drag has reached: on its page the nearest one, above or below the page the
    /// first or the last.
    fn drag_word(&mut self, page: usize, x: f32, y: f32) -> Option<usize> {
        let row = self.layout.rows.get(page).copied()?;
        let text = self.doc.text(page)?;
        let cy = self.scroll + y;
        if cy < row.y {
            return Some(0);
        }
        if cy > row.y + row.h {
            return text.words.len().checked_sub(1);
        }
        let (vw, _) = self.view_px();
        let left = self.layout.origin_x(page, vw, self.scroll_x);
        let size = self.doc.sizes.get(page).copied()?;
        let p = px_to_page((x - left, cy - row.y), size, self.scale, self.rot);
        text.nearest_word(p)
    }

    /// The press ends: what was selected stays, a press without a drag selected nothing.
    fn on_up(&mut self) -> Result<(), UiError> {
        self.drag = None;
        Ok(())
    }

    // ---- touch ----------------------------------------------------------------------------

    /// A touch event on the page area: one finger pans, two fingers pinch.
    pub fn touch(&mut self, event: Event) -> Result<(), UiError> {
        if self.ask.is_some() || self.doc.state != crate::doc::DocState::Open {
            return Ok(());
        }
        if let Event::TouchDown { pos, .. } = event {
            if !self.inside(self.nodes.page, pos) {
                return Ok(());
            }
        }
        let s = self.ui.env().scale;
        match self.touches.handle(&event) {
            Some(Gesture::Pan { dx, dy }) => self.scroll_by(-dx * s, -dy * s),
            Some(Gesture::PinchStart) => {
                self.pinch_from = self.zoom_percent();
                Ok(())
            }
            Some(Gesture::PinchMove { start, dist }) => {
                let to = pinch_percent(self.pinch_from, start, dist);
                if to == self.zoom_percent() {
                    return Ok(());
                }
                self.set_fit(Fit::Zoom(to))
            }
            Some(Gesture::PinchEnd) | None => Ok(()),
        }
    }

    // ---- the widgets ----------------------------------------------------------------------

    /// The actions the widget tree reported (clicks, typed text, chosen list items).
    pub fn actions(&mut self, actions: Vec<Action>) -> Result<(), UiError> {
        for a in actions {
            self.action(a)?;
        }
        Ok(())
    }

    fn action(&mut self, a: Action) -> Result<(), UiError> {
        let node = Some(a.node);
        let n = &self.nodes;
        let clicked = matches!(a.kind, ActionKind::Activated);
        let entered = matches!(a.kind, ActionKind::Submitted);
        if node == n.dialog && matches!(a.kind, ActionKind::Closed) {
            self.ask = None;
            return self.rebuild();
        }
        if clicked && (node == n.open || node == n.empty_open) {
            self.ask = Some(Ask::Open);
            return self.rebuild();
        }
        if clicked && node == n.find {
            return self.open_search();
        }
        if clicked && node == n.zoom_out {
            return self.zoom_by(-1);
        }
        if clicked && node == n.zoom_in {
            return self.zoom_by(1);
        }
        if clicked && node == n.fit_width {
            return self.set_fit(Fit::Width);
        }
        if clicked && node == n.fit_page {
            return self.set_fit(Fit::Page);
        }
        if clicked && node == n.rotate {
            return self.rotate();
        }
        self.action_more(a, clicked, entered)
    }

    fn action_more(&mut self, a: Action, clicked: bool, entered: bool) -> Result<(), UiError> {
        let node = Some(a.node);
        let n = &self.nodes;
        let typed = self.ui.text(a.node).map(str::to_owned).unwrap_or_default();
        if entered && node == n.page_field {
            return self.go_to_field(&typed);
        }
        if matches!(a.kind, ActionKind::TextChanged) && node == n.search_field {
            return self.start_search(&typed);
        }
        if entered && node == n.search_field {
            return self.step_hit(1);
        }
        if clicked && node == n.search_next {
            return self.step_hit(1);
        }
        if clicked && node == n.search_prev {
            return self.step_hit(-1);
        }
        if clicked && node == n.search_close {
            return self.close_search();
        }
        if clicked && (node == n.pages_tab || node == n.outline_tab) {
            self.panel = if node == n.pages_tab { PanelMode::Pages } else { PanelMode::Outline };
            return self.rebuild();
        }
        if let (ActionKind::Selected(i), true) = (a.kind, node == n.outline) {
            let page = self.doc.outline.get(i).map(|o| o.page);
            return page.map_or(Ok(()), |p| self.goto(p));
        }
        self.action_dialog(a, clicked, entered)
    }

    fn action_dialog(&mut self, a: Action, clicked: bool, entered: bool) -> Result<(), UiError> {
        let node = Some(a.node);
        let n = &self.nodes;
        if (clicked && (node == n.open_cancel || node == n.password_cancel))
            || matches!(a.kind, ActionKind::Closed) && node == n.dialog
        {
            self.ask = None;
            return self.rebuild();
        }
        if (clicked && node == n.open_ok) || (entered && node == n.open_field) {
            let entry = n.open_field.and_then(|f| self.ui.text(f).ok()).unwrap_or("").to_owned();
            return self.open_entry(&entry);
        }
        if (clicked && node == n.password_ok) || (entered && node == n.password_field) {
            let pw = n.password_field.and_then(|f| self.ui.text(f).ok()).unwrap_or("").to_owned();
            return self.retry_password(&pw);
        }
        if let (ActionKind::Selected(i), true) = (a.kind, node == n.recents) {
            let entry = self.recents.entries.get(i).cloned();
            return entry.map_or(Ok(()), |e| self.open_entry(&e));
        }
        Ok(())
    }

    /// The page number typed in the field (digits of any script), clamped to the document.
    fn go_to_field(&mut self, typed: &str) -> Result<(), UiError> {
        let n = self.doc.sizes.len();
        match to_western_digits(typed.trim()).parse::<usize>() {
            Ok(p) if n > 0 => self.goto(p.clamp(1, n) - 1),
            _ => self.sync_chrome(),
        }
    }
}
