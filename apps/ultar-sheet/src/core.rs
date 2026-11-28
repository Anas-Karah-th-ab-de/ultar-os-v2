//! The state of a sheet window without any widgets: the workbook, the selection, the cell
//! being edited, and the keys that move and edit. The window ([`crate::app`]) draws it and
//! feeds it input; tests drive it directly.

use crate::book::Book;
use crate::draw::Editing;
use crate::frame::{reveal, Frame};
use crate::sel::Sel;
use ultar_office::sheet::check_formula;
use ultar_office::sheet_refs::{MAX_COLS, MAX_ROWS};
use ultar_ui::event::{Key, Modifiers};
use ultar_ui::widgets::Label;

/// What the window has to do after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fx {
    /// Nothing more.
    None,
    /// Offer this text on the clipboard.
    Copy(String),
    /// Ask for the clipboard text.
    Paste,
    /// Save (ask for a name first when there is none).
    Save,
    /// Open the find bar.
    Find,
    /// Open the find-and-replace bar.
    Replace,
    /// Put the focus into the formula bar.
    FormulaBar,
}

/// The sheet window's state.
pub struct Core {
    /// The workbook.
    pub book: Book,
    /// The selection.
    pub sel: Sel,
    /// First scrolled row and column.
    pub first: (u32, u32),
    /// The cell being edited.
    pub edit: Option<Editing>,
    /// What the last copy took (for pasting formulas and formats back).
    pub clip: Option<crate::ops::Clip>,
    /// Find and replace.
    pub find: crate::ops::Find,
    /// The last message.
    pub message: Option<Label>,
    /// Touch: the next tap extends the selection (there is no Shift on a phone).
    pub extend: bool,
    /// Selection and scroll of every sheet.
    pub views: Vec<(Sel, (u32, u32))>,
    /// Size of the grid in points.
    pub view: (f32, f32),
    /// Touch sizes.
    pub touch: bool,
    /// An action of the assistant that waits for the user's yes.
    pub pending: Option<crate::agent::Request>,
}

impl Core {
    /// A window state for `book`.
    pub fn new(book: Book) -> Core {
        Core {
            book,
            sel: Sel::default(),
            first: (0, 0),
            edit: None,
            clip: None,
            find: crate::ops::Find::default(),
            message: None,
            extend: false,
            views: Vec::new(),
            view: (800.0, 500.0),
            touch: false,
            pending: None,
        }
    }

    /// The geometry of the visible part.
    pub fn frame(&self) -> Frame {
        Frame::of(self.book.sheet(), self.first, self.view.0, self.view.1, self.touch)
    }

    /// Scrolls so that the active cell is visible.
    pub fn reveal(&mut self) {
        let at = self.sel.follow();
        reveal(self.book.sheet(), &mut self.first, at, self.view.0, self.view.1, self.touch);
    }

    /// Says `key` in the status line.
    pub fn say(&mut self, key: &'static str) {
        self.message = Some(Label::key(key));
    }

    /// The raw content of the active cell (what the formula bar shows).
    pub fn raw(&self) -> String {
        self.book.sheet().get(self.sel.active.0, self.sel.active.1).to_owned()
    }

    /// The name of the active cell, `B7`.
    pub fn cell_name(&self) -> String {
        ultar_office::sheet::cell_name(self.sel.active.0, self.sel.active.1)
    }

    /// Switches to sheet `i`, keeping the selection of the one left.
    pub fn show_sheet(&mut self, i: usize) {
        if i >= self.book.wb.sheets.len() || i == self.book.active {
            return;
        }
        self.cancel();
        let n = self.book.wb.sheets.len();
        self.views.resize(n.max(self.views.len()), (Sel::default(), (0, 0)));
        self.views[self.book.active] = (self.sel, self.first);
        self.book.active = i;
        (self.sel, self.first) = self.views[i];
        self.reveal();
    }

    // ---------------------------------------------------------------- editing a cell

    /// Starts editing the active cell; `text` replaces the content (typing over a cell), or
    /// the content is kept (F2, double click).
    pub fn begin_edit(&mut self, text: Option<&str>) {
        let at = self.sel.active;
        let text = text.map_or_else(|| self.raw(), str::to_owned);
        let caret = text.len();
        self.edit = Some(Editing { at, text, caret });
    }

    /// Gives up the edit.
    pub fn cancel(&mut self) {
        self.edit = None;
    }

    /// Writes the text of the edit into its cell. `false` (and a message) for a formula that
    /// does not parse; the edit stays open.
    pub fn commit(&mut self) -> bool {
        let Some(e) = self.edit.take() else {
            return true;
        };
        if e.text.starts_with('=') {
            if let Err(why) = check_formula(&e.text) {
                self.message = Some(Label::key_with("sheet.bad_formula", &[("why", &why)]));
                self.edit = Some(e);
                return false;
            }
        }
        self.book.set(e.at.0, e.at.1, &e.text);
        true
    }

    /// Types into the edit; returns `true` when the key was used.
    pub fn edit_key(&mut self, key: Key, m: Modifiers) -> bool {
        let Some(e) = self.edit.as_mut() else {
            return false;
        };
        let prev = |t: &str, i: usize| t[..i].char_indices().next_back().map_or(0, |(p, _)| p);
        let next = |t: &str, i: usize| t[i..].chars().next().map_or(i, |c| i + c.len_utf8());
        match key {
            Key::Char(c) if !m.command() => {
                e.text.insert(e.caret, c);
                e.caret += c.len_utf8();
            }
            Key::Space if !m.command() => {
                e.text.insert(e.caret, ' ');
                e.caret += 1;
            }
            Key::Backspace => {
                let p = prev(&e.text, e.caret);
                e.text.replace_range(p..e.caret, "");
                e.caret = p;
            }
            Key::Delete => {
                let n = next(&e.text, e.caret);
                e.text.replace_range(e.caret..n, "");
            }
            Key::ArrowLeft => e.caret = prev(&e.text, e.caret),
            Key::ArrowRight => e.caret = next(&e.text, e.caret),
            Key::Home => e.caret = 0,
            Key::End => e.caret = e.text.len(),
            Key::Escape => self.edit = None,
            Key::Enter => {
                if self.commit() {
                    self.sel.advance(true, m.shift);
                    self.reveal();
                }
            }
            Key::Tab => {
                if self.commit() {
                    self.sel.advance(false, m.shift);
                    self.reveal();
                }
            }
            Key::ArrowUp | Key::ArrowDown => {
                if self.commit() {
                    self.sel.step(if key == Key::ArrowUp { -1 } else { 1 }, 0, false);
                    self.reveal();
                }
            }
            _ => return false,
        }
        true
    }

    /// Inserts pasted text into the edit.
    pub fn edit_paste(&mut self, text: &str) {
        if let Some(e) = self.edit.as_mut() {
            let one_line = text.replace(['\r', '\n'], " ");
            e.text.insert_str(e.caret, &one_line);
            e.caret += one_line.len();
        }
    }

    // ---------------------------------------------------------------- moving

    fn filled(&self, r: u32, c: u32) -> bool {
        !self.book.sheet().get(r, c).is_empty()
    }

    /// The end of the run of data in a direction (Ctrl+arrow).
    fn jump(&self, dr: i64, dc: i64) -> (u32, u32) {
        let (rows, cols) = self.book.sheet().extent();
        let (mut r, mut c) = self.sel.active;
        let (max_r, max_c) = (MAX_ROWS - 1, MAX_COLS - 1);
        let at = |r: u32, c: u32, dr: i64, dc: i64| {
            let (nr, nc) = (i64::from(r) + dr, i64::from(c) + dc);
            (nr >= 0 && nc >= 0 && nr <= i64::from(max_r) && nc <= i64::from(max_c)).then_some((nr as u32, nc as u32))
        };
        let limit = |r: u32, c: u32| match (dr, dc) {
            (1, _) => (max_r, c),
            (-1, _) => (0, c),
            (_, 1) => (r, max_c),
            _ => (r, 0),
        };
        let Some((nr, nc)) = at(r, c, dr, dc) else {
            return (r, c);
        };
        let run = self.filled(r, c) && self.filled(nr, nc);
        let guard = u64::from(rows) + u64::from(cols) + 4;
        for _ in 0..guard {
            let Some((nr, nc)) = at(r, c, dr, dc) else {
                return (r, c);
            };
            if run {
                if !self.filled(nr, nc) {
                    return (r, c);
                }
                (r, c) = (nr, nc);
            } else {
                (r, c) = (nr, nc);
                if self.filled(r, c) {
                    return (r, c);
                }
            }
        }
        limit(r, c)
    }

    /// Rows that fit the view, for Page Up and Page Down.
    fn page(&self) -> i64 {
        (self.frame().rows.len() as i64 - 2).max(1)
    }

    /// A key on the grid (not in an edit). Returns what the window has to do.
    pub fn key(&mut self, key: Key, m: Modifiers) -> Fx {
        if self.edit.is_some() {
            self.edit_key(key, m);
            return Fx::None;
        }
        let ext = m.shift;
        let ctrl = m.ctrl || m.meta;
        let go = |core: &mut Core, dr: i64, dc: i64| {
            if ctrl {
                let (r, c) = core.jump(dr, dc);
                if ext {
                    core.sel.extend_to(r, c);
                } else {
                    core.sel.select(r, c);
                }
            } else {
                // the arrows are physical: in a right-to-left sheet "left" is the next column
                let dc = if core.book.sheet().rtl { -dc } else { dc };
                core.sel.step(dr, dc, ext || core.extend);
            }
        };
        match key {
            Key::ArrowUp => go(self, -1, 0),
            Key::ArrowDown => go(self, 1, 0),
            Key::ArrowLeft => go(self, 0, -1),
            Key::ArrowRight => go(self, 0, 1),
            Key::PageUp => {
                let n = self.page();
                self.sel.step(-n, 0, ext);
            }
            Key::PageDown => {
                let n = self.page();
                self.sel.step(n, 0, ext);
            }
            Key::Home if ctrl => self.sel.select(0, 0),
            Key::Home => {
                let r = self.sel.active.0;
                if ext {
                    self.sel.extend_to(r, 0);
                } else {
                    self.sel.select(r, 0);
                }
            }
            Key::End if ctrl => {
                let (rows, cols) = self.book.sheet().extent();
                self.sel.select(rows.saturating_sub(1), cols.saturating_sub(1));
            }
            Key::Enter => {
                self.sel.advance(true, ext);
            }
            Key::Tab => {
                self.sel.advance(false, ext);
            }
            Key::F(2) => {
                self.begin_edit(None);
                return Fx::None;
            }
            Key::Delete | Key::Backspace => self.clear_selection(),
            Key::Escape => {
                self.find.open = false;
                self.sel.select(self.sel.active.0, self.sel.active.1);
            }
            Key::Char(c) if ctrl => return self.shortcut(c.to_ascii_lowercase(), ext),
            Key::Char(c) if !m.command() && !c.is_control() => {
                self.begin_edit(Some(""));
                self.edit_key(Key::Char(c), Modifiers::NONE);
                return Fx::None;
            }
            _ => return Fx::None,
        }
        self.reveal();
        Fx::None
    }

    fn shortcut(&mut self, c: char, shift: bool) -> Fx {
        match c {
            'z' if shift => {
                self.redo();
            }
            'z' => {
                self.undo();
            }
            'y' => {
                self.redo();
            }
            'c' => return Fx::Copy(self.copy()),
            'x' => return Fx::Copy(self.cut()),
            'v' => return Fx::Paste,
            'a' => {
                let ext = self.book.sheet().extent();
                self.sel.all(ext);
            }
            'b' => self.toggle_bold(),
            'i' => self.toggle_italic(),
            'd' => self.fill(true),
            'r' => self.fill(false),
            's' => return Fx::Save,
            'f' => return Fx::Find,
            'h' => return Fx::Replace,
            _ => {}
        }
        self.reveal();
        Fx::None
    }

    /// Undo (the changed sheet comes to the front).
    pub fn undo(&mut self) {
        self.cancel();
        let before = self.book.active;
        if self.book.undo() {
            self.after_history(before);
        }
    }

    /// Redo.
    pub fn redo(&mut self) {
        self.cancel();
        let before = self.book.active;
        if self.book.redo() {
            self.after_history(before);
        }
    }

    fn after_history(&mut self, before: usize) {
        if self.book.active != before {
            (self.sel, self.first) = (Sel::default(), (0, 0));
        }
        let n = self.book.wb.sheets.len();
        self.views.truncate(n);
        let (r, c) = self.book.sheet().extent();
        let (cr, cc) = self.sel.active;
        self.sel.select(cr.min(r.max(1) + 1000), cc.min(c.max(1) + 100));
    }
}
