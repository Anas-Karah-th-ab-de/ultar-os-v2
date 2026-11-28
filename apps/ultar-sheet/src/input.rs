//! Keys, mouse, touch and wheel on the grid, the clipboard, and the assistant's requests.

use crate::agent::{Reply, Request};
use crate::app::{Out, SheetApp, State};
use crate::ask::Ask;
use crate::core::Fx;
use crate::frame::{col_points, row_points, Hit};
use crate::sel::Span;
use std::time::{Duration, Instant};
use ultar_ui::event::{Event, Key, Modifiers, PointerButton};
use ultar_ui::geom::Point;
use ultar_ui::ui::UiError;

/// Two presses within this time on one cell are a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(450);
/// A finger that moves further than this many points scrolls instead of selecting.
const TOUCH_SLOP: f32 = 8.0;

/// What a pressed pointer is doing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Drag {
    /// Nothing.
    None,
    /// Selecting a range.
    Select,
    /// Selecting rows from a row number.
    Rows,
    /// Selecting columns from a column letter.
    Cols,
    /// Dragging the border of a column: column, pointer start, width at the start.
    ColSize(u32, f32, f32),
    /// Dragging the border of a row.
    RowSize(u32, f32, f32),
    /// A finger down: start, last position, moved far enough to scroll, what it touched.
    Finger(Point, Point, bool, Hit),
}

impl SheetApp {
    /// `true` while the grid has the keyboard (keys then bypass the widgets).
    pub fn grid_focused(&self) -> bool {
        self.ask.is_none()
            && self.state == State::Ready
            && self.ui.focused().is_some()
            && self.ui.focused() == self.nodes.grid
    }

    // ---------------------------------------------------------------- keys

    /// A key the widgets did not use (with the grid focused, every key comes here).
    pub fn key(&mut self, event: Event) -> Result<(), UiError> {
        let Event::Key { key, modifiers } = event else {
            return Ok(());
        };
        self.mods = modifiers;
        if self.ask.is_some() || self.state != State::Ready {
            return Ok(());
        }
        let ctrl = modifiers.ctrl || modifiers.meta;
        if !self.grid_focused() {
            // a field has the keyboard: only the shortcuts of the whole window
            match key {
                Key::Escape => {
                    self.core.cancel();
                    self.focus_grid();
                    return self.refresh();
                }
                Key::Char(c) if ctrl && c.eq_ignore_ascii_case(&'s') => return self.save(crate::ask::Then::Nothing),
                Key::Char(c) if ctrl && c.eq_ignore_ascii_case(&'f') => return self.open_find(false),
                Key::Char(c) if ctrl && c.eq_ignore_ascii_case(&'h') => return self.open_find(true),
                _ => return Ok(()),
            }
        }
        if key == Key::Escape && self.core.edit.is_none() && self.core.find.open {
            self.core.find.open = false;
            return self.rebuild();
        }
        let fx = self.core.key(key, modifiers);
        self.follow(fx)
    }

    fn follow(&mut self, fx: Fx) -> Result<(), UiError> {
        match fx {
            Fx::None => {}
            Fx::Copy(text) => self.out.push(Out::Copy(text)),
            Fx::Paste => self.out.push(Out::Paste),
            Fx::Save => return self.save(crate::ask::Then::Nothing),
            Fx::Find => return self.open_find(false),
            Fx::Replace => return self.open_find(true),
            Fx::FormulaBar => {
                if let Some(f) = self.nodes.formula {
                    self.ui.focus(f)?;
                }
            }
        }
        if self.core.sel.span != Span::Cells && self.core.sel.single() {
            // nothing to do: kept for clarity of the selection kinds
        }
        self.refresh()
    }

    /// Clipboard text that arrived.
    pub fn pasted(&mut self, text: &str) -> Result<(), UiError> {
        if self.ask.is_some() || self.state != State::Ready {
            return Ok(());
        }
        self.core.paste(text);
        self.refresh()
    }

    // ---------------------------------------------------------------- the pointer

    fn local(&self, pos: Point) -> Option<Point> {
        let rect = self.ui.rect(self.nodes.grid?)?;
        Some(Point::new(pos.x - rect.x, pos.y - rect.y))
    }

    fn inside(&self, pos: Point) -> bool {
        self.nodes.grid.and_then(|g| self.ui.rect(g)).is_some_and(|r| r.contains(pos))
    }

    /// A pointer event (the widgets saw it too): selection, resizing and scrolling.
    pub fn pointer(&mut self, event: Event) -> Result<(), UiError> {
        if self.ask.is_some() || self.state != State::Ready {
            return Ok(());
        }
        match event {
            Event::Modifiers { modifiers } => self.mods = modifiers,
            Event::PointerDown { pos, button: PointerButton::Primary } if self.inside(pos) => {
                self.press(pos)?;
            }
            Event::PointerMove { pos } => self.moved(pos)?,
            Event::PointerUp { .. } => self.released(),
            Event::TouchDown { pos, .. } if self.inside(pos) => self.finger_down(pos)?,
            Event::TouchMove { pos, .. } => self.finger_move(pos)?,
            Event::TouchUp { pos, .. } => self.finger_up(pos)?,
            Event::TouchCancel { .. } => self.drag = Drag::None,
            Event::Scroll { pos, lines } if self.inside(pos) => self.scroll(lines)?,
            _ => {}
        }
        Ok(())
    }

    fn hit(&self, pos: Point) -> Option<Hit> {
        let l = self.local(pos)?;
        Some(self.core.frame().hit(l.x, l.y))
    }

    fn press(&mut self, pos: Point) -> Result<(), UiError> {
        let Some(hit) = self.hit(pos) else {
            return Ok(());
        };
        if self.core.edit.is_some() && !matches!(hit, Hit::None) && !self.core.commit() {
            return self.refresh();
        }
        self.focus_grid();
        let shift = self.mods.shift || self.core.extend;
        let sheet_rtl = self.core.book.sheet().rtl;
        match hit {
            Hit::Corner => {
                let ext = self.core.book.sheet().extent();
                self.core.sel.all(ext);
            }
            Hit::Col(c) => {
                self.core.sel.col(c, shift);
                self.drag = Drag::Cols;
            }
            Hit::Row(r) => {
                self.core.sel.row(r, shift);
                self.drag = Drag::Rows;
            }
            Hit::ColEdge(c) => {
                let w = col_points(self.core.book.sheet(), c);
                self.drag = Drag::ColSize(c, pos.x, w);
                let _ = sheet_rtl;
            }
            Hit::RowEdge(r) => {
                let h = row_points(self.core.book.sheet(), r, self.core.touch);
                self.drag = Drag::RowSize(r, pos.y, h);
            }
            Hit::Cell(r, c) => {
                let now = Instant::now();
                let double = self.last_click.is_some_and(|(t, at)| at == (r, c) && now.duration_since(t) < DOUBLE_CLICK);
                if shift {
                    self.core.sel.extend_to(r, c);
                } else {
                    self.core.sel.select(r, c);
                }
                self.drag = Drag::Select;
                self.last_click = Some((now, (r, c)));
                if double {
                    self.core.begin_edit(None);
                    self.drag = Drag::None;
                }
            }
            Hit::None => {}
        }
        self.refresh()
    }

    fn moved(&mut self, pos: Point) -> Result<(), UiError> {
        match self.drag {
            Drag::None | Drag::Finger(..) => Ok(()),
            Drag::Select | Drag::Rows | Drag::Cols => {
                let Some(l) = self.local(pos) else {
                    return Ok(());
                };
                let frame = self.core.frame();
                let (w, h) = self.core.view;
                // beyond the edge the grid scrolls
                if l.y > h {
                    self.core.first.0 = self.core.first.0.saturating_add(1);
                } else if l.y < frame.hdr_h && self.core.first.0 > 0 {
                    self.core.first.0 -= 1;
                }
                let toward_end = if frame.rtl { l.x < frame.hdr_w } else { l.x > w };
                let toward_start = if frame.rtl { l.x > w - frame.hdr_w } else { l.x < frame.hdr_w };
                if toward_end {
                    self.core.first.1 = self.core.first.1.saturating_add(1);
                } else if toward_start && self.core.first.1 > 0 {
                    self.core.first.1 -= 1;
                }
                let frame = self.core.frame();
                let (x, y) = (l.x.clamp(frame.hdr_w + 1.0, w - 1.0), l.y.clamp(frame.hdr_h + 1.0, h - 1.0));
                match (self.drag, frame.hit(x, y)) {
                    (Drag::Select, Hit::Cell(r, c)) => self.core.sel.extend_to(r, c),
                    (Drag::Rows, Hit::Cell(r, _)) => self.core.sel.row(r, true),
                    (Drag::Cols, Hit::Cell(_, c)) => self.core.sel.col(c, true),
                    _ => {}
                }
                self.draw_grid()
            }
            Drag::ColSize(c, x0, w0) => {
                let dx = if self.core.book.sheet().rtl { x0 - pos.x } else { pos.x - x0 };
                self.core.set_col_width(c, w0 + dx);
                self.draw_grid()
            }
            Drag::RowSize(r, y0, h0) => {
                self.core.set_row_height(r, h0 + (pos.y - y0));
                self.draw_grid()
            }
        }
    }

    fn released(&mut self) {
        self.drag = Drag::None;
    }

    /// Wheel: rows (columns with Shift).
    fn scroll(&mut self, lines: f32) -> Result<(), UiError> {
        let n = (lines * 3.0).round() as i64;
        self.scroll_by(if self.mods.shift { 0 } else { n }, if self.mods.shift { n } else { 0 })
    }

    /// Scrolls the grid by rows and columns.
    pub fn scroll_by(&mut self, rows: i64, cols: i64) -> Result<(), UiError> {
        let sh = self.core.book.sheet();
        let (used_r, used_c) = sh.extent();
        let (fr, fc) = (sh.frozen_rows, sh.frozen_cols);
        let max_r = i64::from(used_r.max(self.core.sel.active.0) + 50);
        let max_c = i64::from(used_c.max(self.core.sel.active.1) + 10);
        let dir = if sh.rtl && false { -1 } else { 1 };
        self.core.first.0 = (i64::from(self.core.first.0) + rows).clamp(i64::from(fr), max_r.max(i64::from(fr))) as u32;
        self.core.first.1 = (i64::from(self.core.first.1) + cols * dir).clamp(i64::from(fc), max_c.max(i64::from(fc))) as u32;
        self.draw_grid()
    }

    // ---------------------------------------------------------------- a finger

    fn finger_down(&mut self, pos: Point) -> Result<(), UiError> {
        let hit = self.hit(pos).unwrap_or(Hit::None);
        if self.core.edit.is_some() && !self.core.commit() {
            return self.refresh();
        }
        self.focus_grid();
        match hit {
            Hit::ColEdge(c) => {
                let w = col_points(self.core.book.sheet(), c);
                self.drag = Drag::ColSize(c, pos.x, w);
            }
            Hit::RowEdge(r) => {
                let h = row_points(self.core.book.sheet(), r, true);
                self.drag = Drag::RowSize(r, pos.y, h);
            }
            _ => self.drag = Drag::Finger(pos, pos, false, hit),
        }
        Ok(())
    }

    fn finger_move(&mut self, pos: Point) -> Result<(), UiError> {
        match self.drag {
            Drag::Finger(start, last, scrolling, hit) => {
                let far = (pos.x - start.x).abs() > TOUCH_SLOP || (pos.y - start.y).abs() > TOUCH_SLOP;
                if !(scrolling || far) {
                    return Ok(());
                }
                // drag the content: the finger moves up, the sheet moves up (rows go on)
                let row_h = row_points(self.core.book.sheet(), self.core.first.0, true).max(1.0);
                let dy = ((last.y - pos.y) / row_h).trunc() as i64;
                let col_w = col_points(self.core.book.sheet(), self.core.first.1).max(1.0);
                let sign = if self.core.book.sheet().rtl { -1.0 } else { 1.0 };
                let dx = (sign * (last.x - pos.x) / col_w).trunc() as i64;
                let moved = Point::new(
                    if dx != 0 { pos.x } else { last.x },
                    if dy != 0 { pos.y } else { last.y },
                );
                self.drag = Drag::Finger(start, moved, true, hit);
                if dy != 0 || dx != 0 {
                    self.scroll_by(dy, dx)?;
                }
                Ok(())
            }
            Drag::ColSize(..) | Drag::RowSize(..) => self.moved(pos),
            _ => Ok(()),
        }
    }

    fn finger_up(&mut self, _pos: Point) -> Result<(), UiError> {
        let drag = std::mem::replace(&mut self.drag, Drag::None);
        if let Drag::Finger(_, _, false, hit) = drag {
            // a tap
            let ext = self.core.extend;
            match hit {
                Hit::Cell(r, c) => {
                    if ext {
                        self.core.sel.extend_to(r, c);
                    } else {
                        let again = self.core.sel.single() && self.core.sel.active == (r, c);
                        self.core.sel.select(r, c);
                        if again {
                            // a second tap on the selected cell edits it
                            self.core.begin_edit(None);
                        }
                    }
                }
                Hit::Row(r) => self.core.sel.row(r, ext),
                Hit::Col(c) => self.core.sel.col(c, ext),
                Hit::Corner => {
                    let e = self.core.book.sheet().extent();
                    self.core.sel.all(e);
                }
                _ => {}
            }
            return self.refresh();
        }
        Ok(())
    }

    // ---------------------------------------------------------------- the assistant

    /// A request of the assistant. Reading answers at once; writing and adding a sheet ask
    /// the user first (the answer comes with [`SheetApp::take_agent_reply`]).
    pub fn agent(&mut self, req: Request) -> Result<Reply, UiError> {
        let what = match &req {
            Request::WriteRange { range, values } => {
                let n: usize = values.iter().map(Vec::len).sum();
                Some(Label_what("sheet.agent.write", &[("n", &n.to_string()), ("range", range)], self))
            }
            Request::AddSheet { name } => Some(Label_what("sheet.agent.add", &[("name", name)], self)),
            _ => None,
        };
        let reply = self.core.agent(req);
        match (&reply, what) {
            (Reply::Confirm, Some(w)) => {
                self.ask = Some(Ask::Agent(w));
                self.rebuild()?;
            }
            (Reply::Open(uri), _) => {
                let uri = uri.clone();
                self.open_uri(&uri)?;
            }
            _ => {}
        }
        Ok(reply)
    }

    /// The answer to the last request that waited for the user.
    pub fn take_agent_reply(&mut self) -> Option<Reply> {
        self.agent_reply.take()
    }
}

#[allow(non_snake_case)]
fn Label_what(key: &str, args: &[(&str, &str)], app: &SheetApp) -> String {
    ultar_ui::widgets::Label::key_with(key, args).resolve(app.ui.translator()).into_owned()
}

/// Modifiers of a plain key press (tests and the window).
pub const NO_MODS: Modifiers = Modifiers::NONE;
