//! What a key does. Arrow keys are physical: in a right-to-left line the left arrow moves on
//! to the next letter, in a left-to-right line back. Up and down follow the layout (and keep
//! their column), Home and End go to the start/end of the line. `Ctrl+B/I/U` toggle the
//! formats, `Ctrl+S` saves, `Ctrl+N` starts a new document, `Ctrl+A/C/X/V/Z/Y/F` as usual,
//! `Ctrl+Enter` breaks the page, `Ctrl+Plus/Minus` zoom, `Ctrl+Arrow` moves by words.

use crate::action::Out;
use crate::app::App;
use crate::commands::Cmd;
use crate::state::Cursor;
use ultar_ui::event::{Event, Key, Modifiers};
use ultar_ui::ui::UiError;

impl App {
    /// A key the widgets did not use.
    pub fn key(&mut self, event: Event) -> Result<(), UiError> {
        let Event::Key { key, modifiers: m } = event else {
            return Ok(());
        };
        if self.ask.is_some() {
            return Ok(());
        }
        if m.ctrl && !m.alt {
            return self.ctrl_key(key, m);
        }
        if m.alt || m.meta {
            return Ok(());
        }
        match key {
            Key::Char(c) => self.typed(&c.to_string()),
            Key::Space => self.typed(" "),
            Key::Enter => self.edit(|d| d.newline()),
            Key::Tab if m.shift => self.do_cmd(Cmd::Outdent),
            Key::Tab
                if self.doc.caret_para().is_some_and(|p| {
                    p.style == ultar_office::doc::Style::Bullet
                        || p.style == ultar_office::doc::Style::Number
                }) =>
            {
                self.do_cmd(Cmd::Indent)
            }
            Key::Tab => self.typed("    "),
            Key::Backspace => self.edit(|d| d.backspace()),
            Key::Delete => self.edit(|d| d.delete()),
            _ => self.navigate(key, m.shift, false),
        }
    }

    fn typed(&mut self, s: &str) -> Result<(), UiError> {
        self.inp.goal_x = None;
        self.edit(|d| d.insert(s))
    }

    pub(crate) fn edit(&mut self, f: impl FnOnce(&mut crate::state::Doc)) -> Result<(), UiError> {
        f(&mut self.doc);
        self.changed()
    }

    fn ctrl_key(&mut self, key: Key, m: Modifiers) -> Result<(), UiError> {
        match key {
            Key::Char(c) => self.ctrl_char(c.to_ascii_lowercase(), m.shift),
            Key::Enter => self.do_cmd(Cmd::PageBreak),
            k if is_navigation(k) => self.navigate(key, m.shift, true),
            _ => Ok(()),
        }
    }

    fn ctrl_char(&mut self, c: char, shift: bool) -> Result<(), UiError> {
        match c {
            's' => self.save(false),
            'n' => self.new_doc(),
            'f' => self.do_cmd(Cmd::Find),
            'a' => {
                self.doc.select_all();
                self.moved()
            }
            'z' if shift => self.do_cmd(Cmd::Redo),
            'z' => self.do_cmd(Cmd::Undo),
            'y' => self.do_cmd(Cmd::Redo),
            'b' => self.do_cmd(Cmd::Bold),
            'i' => self.do_cmd(Cmd::Italic),
            'u' => self.do_cmd(Cmd::Underline),
            'e' => self.do_cmd(Cmd::AlignCenter),
            'j' => self.do_cmd(Cmd::AlignJustify),
            '=' | '+' => self.do_cmd(Cmd::ZoomIn),
            '-' => self.do_cmd(Cmd::ZoomOut),
            'c' => self.copy(false),
            'x' => self.copy(true),
            'v' => {
                self.io.out.push(Out::Paste);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Copies the selection (plain text to the surface clipboard, the formatted blocks to
    /// the app's own buffer); `cut` removes it afterwards.
    pub(crate) fn copy(&mut self, cut: bool) -> Result<(), UiError> {
        let text = self.doc.selected_text();
        if text.is_empty() {
            return Ok(());
        }
        self.io.rich = Some((text.clone(), self.doc.copy_blocks()));
        self.io.out.push(Out::Copy(text));
        if cut {
            self.doc.burst_break();
            self.doc.delete_selection();
            self.doc.burst_break();
            return self.changed();
        }
        Ok(())
    }

    /// Clipboard text that arrived: the app's own formatted blocks when it is the text copied
    /// here, otherwise plain text.
    pub fn pasted(&mut self, text: &str) -> Result<(), UiError> {
        if self.ask.is_some() || text.is_empty() {
            return Ok(());
        }
        self.doc.burst_break();
        match self.io.rich.clone() {
            Some((t, blocks)) if t == text => self.doc.paste_blocks(blocks),
            _ => self.doc.insert(text),
        }
        self.doc.burst_break();
        self.changed()
    }

    /// Movement keys; `word` is the Ctrl state.
    fn navigate(&mut self, key: Key, extend: bool, word: bool) -> Result<(), UiError> {
        let rtl = self.caret_rtl();
        let vertical = matches!(
            key,
            Key::ArrowUp | Key::ArrowDown | Key::PageUp | Key::PageDown
        );
        match key {
            k if is_arrow(k) => {
                let on = matches!(key, Key::ArrowLeft) == rtl; // physical
                let step = if on { 1 } else { -1 };
                if word {
                    self.doc.move_word(step, extend);
                } else {
                    self.doc.move_chars(step, extend);
                }
            }
            Key::Home | Key::End => self.home_end(matches!(key, Key::Home), extend, word),
            Key::ArrowUp | Key::ArrowDown => {
                self.vertical(if key == Key::ArrowUp { -1 } else { 1 }, extend, 1)
            }
            Key::PageUp | Key::PageDown => {
                let rows = (self.view_size().1 / self.lay_line_h().max(1.0))
                    .floor()
                    .max(1.0) as isize
                    - 1;
                self.vertical(if key == Key::PageUp { -1 } else { 1 }, extend, rows.max(1));
            }
            Key::Escape => {
                self.doc.anchor = None;
            }
            _ => return Ok(()),
        }
        if !vertical {
            self.inp.goal_x = None;
        }
        self.moved()
    }

    fn home_end(&mut self, home: bool, extend: bool, whole: bool) {
        if whole {
            let last = self.doc.document.blocks.len().saturating_sub(1);
            let target = if home {
                Cursor::default()
            } else {
                let len = self.doc.item_len(last, None).unwrap_or(0);
                Cursor {
                    block: last,
                    cell: None,
                    ch: len,
                }
            };
            self.doc.set_pos(target, extend);
            return;
        }
        self.ensure_layout();
        let c = self.doc.cursor;
        let to = self.lay.as_ref().and_then(|l| {
            if home {
                l.line_start(&c)
            } else {
                l.line_end(&c)
            }
        });
        match to {
            Some(t) => self.doc.set_pos(t, extend),
            None if home => self.doc.home(extend),
            None => self.doc.end(extend),
        }
    }

    /// Moves `rows` lines up or down, keeping the column.
    fn vertical(&mut self, dir: isize, extend: bool, rows: isize) {
        self.ensure_layout();
        let c = self.doc.cursor;
        let Some(lay) = self.lay.as_ref() else {
            return;
        };
        let x = self.inp.goal_x.or_else(|| lay.caret(&c).map(|p| p.x));
        self.inp.goal_x = x;
        let mut at = c;
        for _ in 0..rows {
            match lay.move_vertical(&at, dir, x.unwrap_or(0.0)) {
                Some(n) if n != at => at = n,
                _ => break,
            }
        }
        if at == c && dir < 0 {
            at = Cursor { ch: 0, ..c };
        } else if at == c {
            let len = self.doc.item_len(c.block, c.cell).unwrap_or(0);
            at = Cursor { ch: len, ..c };
        }
        self.doc.set_pos(at, extend);
    }

    /// `true` when the caret's line runs right to left.
    pub(crate) fn caret_rtl(&mut self) -> bool {
        self.ensure_layout();
        let c = self.doc.cursor;
        self.lay
            .as_ref()
            .and_then(|l| {
                l.line_of(&c)
                    .and_then(|i| l.placed.get(i))
                    .map(|p| p.line().rtl)
            })
            .unwrap_or(false)
    }
}

/// Keys that move the caret by the screen direction, line ends included.
fn is_navigation(k: Key) -> bool {
    matches!(k, Key::ArrowLeft | Key::ArrowRight | Key::Home | Key::End) // physical
}

/// The two horizontal arrows: they are physical, not logical.
fn is_arrow(k: Key) -> bool {
    matches!(k, Key::ArrowLeft | Key::ArrowRight) // physical
}
