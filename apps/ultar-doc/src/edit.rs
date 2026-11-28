//! Editing operations of [`Doc`]: typing, deleting, splitting and merging paragraphs,
//! movement, paragraph and character formatting, lists, page break, pictures and the blocks
//! of the app's own rich clipboard. Formatting keeps the runs of mixed Arabic/Latin text
//! (see `ultar_office::doc::edit`).

use crate::state::{Cell, Cursor, Doc};
use ultar_office::doc::{Block, Para, Style};

fn chars_range(s: &str, from: usize, to: usize) -> String {
    s.chars().skip(from).take(to.saturating_sub(from)).collect()
}

fn cut_chars(s: &mut String, from: usize, to: usize) {
    let head = chars_range(s, 0, from);
    let tail: String = s.chars().skip(to).collect();
    *s = head + &tail;
}

impl Doc {
    // ---- typing -----------------------------------------------------------

    /// Inserts text at the caret (replacing a selection); line feeds split paragraphs.
    pub fn insert(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        if self.selection().is_some() {
            self.delete_selection();
        }
        let text = s.replace("\r\n", "\n").replace('\r', "\n");
        for (i, part) in text.split('\n').enumerate() {
            if i > 0 {
                self.newline();
            }
            if !part.is_empty() {
                self.insert_plain(part);
            }
        }
    }

    fn insert_plain(&mut self, s: &str) {
        self.begin();
        let c = self.cursor;
        let look = self.look.take();
        let n = s.chars().count();
        match self.document.blocks.get_mut(c.block) {
            Some(Block::Para(p)) if c.cell.is_none() => {
                p.insert_at(c.ch, s, look.as_ref());
                self.cursor.ch = c.ch + n;
            }
            Some(Block::Table(t)) => {
                let cell = c.cell.unwrap_or_default();
                if let Some(text) = t.get_mut(cell.row).and_then(|r| r.get_mut(cell.col)) {
                    let at = c.ch.min(text.chars().count());
                    let head = chars_range(text, 0, at);
                    let tail: String = text.chars().skip(at).collect();
                    *text = format!("{head}{s}{tail}");
                    self.cursor.ch = at + n;
                }
            }
            Some(Block::Picture(_)) => {
                let at = if c.ch == 0 { c.block } else { c.block + 1 };
                self.document
                    .blocks
                    .insert(at, Block::Para(Para::new(Style::Normal, s)));
                self.cursor = Cursor {
                    block: at,
                    cell: None,
                    ch: n,
                };
            }
            _ => {}
        }
        self.clamp();
    }

    /// Enter: splits the paragraph at the caret. In an empty list item it ends the list; in
    /// a table cell it adds a line to the cell.
    pub fn newline(&mut self) {
        if self.selection().is_some() {
            self.delete_selection();
        }
        self.begin();
        let c = self.cursor;
        match self.document.blocks.get_mut(c.block) {
            Some(Block::Para(p)) if c.cell.is_none() => {
                if p.char_len() == 0 && matches!(p.style, Style::Bullet | Style::Number) {
                    p.style = Style::Normal;
                    p.indent_pt = None;
                    return;
                }
                let tail = p.split_off(c.ch);
                self.document.blocks.insert(c.block + 1, Block::Para(tail));
                self.cursor = Cursor {
                    block: c.block + 1,
                    cell: None,
                    ch: 0,
                };
            }
            Some(Block::Picture(_)) => {
                let at = if c.ch == 0 { c.block } else { c.block + 1 };
                self.document
                    .blocks
                    .insert(at, Block::Para(Para::default()));
                self.cursor = Cursor {
                    block: at + usize::from(c.ch == 0),
                    cell: None,
                    ch: 0,
                };
            }
            _ => {
                self.insert_plain("\n");
                return;
            }
        }
        self.look = None;
        self.clamp();
    }

    /// Backspace: deletes the character before the caret or merges with the previous paragraph.
    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        let c = self.cursor;
        if c.ch > 0 {
            self.begin();
            self.cut_item(c, c.ch - 1, c.ch);
            self.cursor.ch = c.ch - 1;
            return;
        }
        if c.cell.is_some() {
            return;
        }
        self.begin();
        if let Some(Block::Para(p)) = self.document.blocks.get_mut(c.block) {
            if p.style != Style::Normal || p.indent_pt.is_some() {
                p.style = Style::Normal;
                p.indent_pt = None;
                return;
            }
        }
        if c.block == 0 {
            return;
        }
        self.join_backwards(c);
        self.clamp();
    }

    /// Backspace at the start of block `c.block`: merges with the paragraph before, removes a
    /// picture before it or moves the caret into the end of a table before it.
    fn join_backwards(&mut self, c: Cursor) {
        match (
            self.document.blocks.get(c.block - 1),
            self.document.blocks.get(c.block),
        ) {
            (Some(Block::Para(prev)), Some(Block::Para(_))) => {
                let at = prev.char_len();
                if let Block::Para(cur) = self.document.blocks.remove(c.block) {
                    if let Some(Block::Para(prev)) = self.document.blocks.get_mut(c.block - 1) {
                        prev.append(&cur);
                    }
                }
                self.cursor = Cursor {
                    block: c.block - 1,
                    cell: None,
                    ch: at,
                };
            }
            (Some(Block::Picture(_)), _) => {
                self.document.blocks.remove(c.block - 1);
                self.cursor = Cursor {
                    block: c.block - 1,
                    cell: None,
                    ch: 0,
                };
            }
            (Some(Block::Table(_)), _) => {
                let (b, cell) = self
                    .item_before(c.block, None)
                    .unwrap_or((c.block - 1, None));
                let ch = self.item_len(b, cell).unwrap_or(0);
                self.cursor = Cursor { block: b, cell, ch };
            }
            _ => {}
        }
    }

    /// Delete: removes the character after the caret or merges the next paragraph in.
    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        let c = self.cursor;
        let len = self.item_len(c.block, c.cell).unwrap_or(0);
        if matches!(self.document.blocks.get(c.block), Some(Block::Picture(_))) {
            if c.ch == 0 {
                self.begin();
                self.document.blocks.remove(c.block);
                self.clamp();
            }
            return;
        }
        if c.ch < len {
            self.begin();
            self.cut_item(c, c.ch, c.ch + 1);
            return;
        }
        if c.cell.is_some() || c.block + 1 >= self.document.blocks.len() {
            return;
        }
        self.begin();
        match self.document.blocks.get(c.block + 1) {
            Some(Block::Para(_)) => {
                if let Block::Para(next) = self.document.blocks.remove(c.block + 1) {
                    if let Some(Block::Para(p)) = self.document.blocks.get_mut(c.block) {
                        p.append(&next);
                    }
                }
            }
            Some(Block::Picture(_)) => {
                self.document.blocks.remove(c.block + 1);
            }
            _ => {}
        }
        self.clamp();
    }

    /// Removes the characters `from..to` of the item at `c`.
    fn cut_item(&mut self, c: Cursor, from: usize, to: usize) {
        match (self.document.blocks.get_mut(c.block), c.cell) {
            (Some(Block::Para(p)), None) => p.delete_range(from, to),
            (Some(Block::Table(t)), Some(cell)) => {
                if let Some(s) = t.get_mut(cell.row).and_then(|r| r.get_mut(cell.col)) {
                    cut_chars(s, from, to);
                }
            }
            (Some(Block::Picture(_)), None) if from == 0 && to >= 1 => {
                self.document.blocks.remove(c.block);
            }
            _ => {}
        }
    }

    /// Removes the selection and returns whether anything was removed.
    pub fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            return false;
        };
        self.begin();
        if a.block == b.block {
            self.delete_inside(a, b);
        } else {
            self.delete_across(a, b);
        }
        self.cursor = Cursor {
            block: a.block,
            cell: a.cell,
            ch: a.ch,
        };
        self.anchor = None;
        self.look = None;
        self.clamp();
        true
    }

    fn delete_inside(&mut self, a: Cursor, b: Cursor) {
        let Some(Block::Table(t)) = self.document.blocks.get_mut(a.block) else {
            self.cut_item(a, a.ch, b.ch);
            return;
        };
        let (ca, cb) = (a.cell.unwrap_or_default(), b.cell.unwrap_or_default());
        for (r, row) in t.iter_mut().enumerate() {
            for (k, s) in row.iter_mut().enumerate() {
                let at = Cell { row: r, col: k };
                if at < ca || at > cb {
                    continue;
                }
                let n = s.chars().count();
                let from = if at == ca { a.ch } else { 0 };
                let to = if at == cb { b.ch } else { n };
                cut_chars(s, from, to);
            }
        }
    }

    fn delete_across(&mut self, a: Cursor, b: Cursor) {
        let blocks = &mut self.document.blocks;
        let drop_tail = match blocks.get_mut(b.block) {
            Some(Block::Para(p)) => {
                p.delete_range(0, b.ch);
                false
            }
            Some(Block::Picture(_)) => b.ch >= 1,
            _ => false,
        };
        let drop_head = match blocks.get_mut(a.block) {
            Some(Block::Para(p)) => {
                let n = p.char_len();
                p.delete_range(a.ch, n);
                false
            }
            Some(Block::Picture(_)) => a.ch == 0,
            _ => false,
        };
        if drop_tail {
            blocks.remove(b.block);
        }
        let upto = b.block.min(blocks.len());
        blocks.drain(a.block + 1..upto);
        let both_paras = !drop_tail
            && !drop_head
            && matches!(blocks.get(a.block), Some(Block::Para(_)))
            && matches!(blocks.get(a.block + 1), Some(Block::Para(_)));
        if both_paras {
            if let Block::Para(tail) = blocks.remove(a.block + 1) {
                if let Some(Block::Para(head)) = blocks.get_mut(a.block) {
                    head.append(&tail);
                }
            }
        }
        if drop_head && a.block < blocks.len() {
            blocks.remove(a.block);
        }
    }

    // ---- movement ---------------------------------------------------------

    /// Moves the caret by `delta` characters, over paragraph and cell borders.
    pub fn move_chars(&mut self, delta: isize, extend: bool) {
        let mut c = self.cursor;
        for _ in 0..delta.unsigned_abs() {
            let len = self.item_len(c.block, c.cell).unwrap_or(0);
            if delta > 0 {
                if c.ch < len {
                    c.ch += 1;
                } else if let Some((block, cell)) = self.item_after(c.block, c.cell) {
                    c = Cursor { block, cell, ch: 0 };
                } else {
                    break;
                }
            } else if c.ch > 0 {
                c.ch -= 1;
            } else if let Some((block, cell)) = self.item_before(c.block, c.cell) {
                let ch = self.item_len(block, cell).unwrap_or(0);
                c = Cursor { block, cell, ch };
            } else {
                break;
            }
        }
        self.set_pos(c, extend);
    }

    /// Moves to the start of the next word (`dir > 0`) or of this/the previous one.
    pub fn move_word(&mut self, dir: isize, extend: bool) {
        let c = self.cursor;
        let text: Vec<char> = self
            .item_text(c.block, c.cell)
            .unwrap_or_default()
            .chars()
            .collect();
        let mut i = c.ch.min(text.len());
        if dir > 0 {
            if i >= text.len() {
                return self.move_chars(1, extend);
            }
            while i < text.len() && !text[i].is_whitespace() {
                i += 1;
            }
            while i < text.len() && text[i].is_whitespace() {
                i += 1;
            }
        } else {
            if i == 0 {
                return self.move_chars(-1, extend);
            }
            while i > 0 && text[i - 1].is_whitespace() {
                i -= 1;
            }
            while i > 0 && !text[i - 1].is_whitespace() {
                i -= 1;
            }
        }
        self.set_pos(Cursor { ch: i, ..c }, extend);
    }

    /// Start of the item.
    pub fn home(&mut self, extend: bool) {
        let c = self.cursor;
        self.set_pos(Cursor { ch: 0, ..c }, extend);
    }

    /// End of the item.
    pub fn end(&mut self, extend: bool) {
        let c = self.cursor;
        let len = self.item_len(c.block, c.cell).unwrap_or(0);
        self.set_pos(Cursor { ch: len, ..c }, extend);
    }

    /// Selects the word under the caret.
    pub fn select_word(&mut self) {
        let c = self.cursor;
        let text: Vec<char> = self
            .item_text(c.block, c.cell)
            .unwrap_or_default()
            .chars()
            .collect();
        let word = |ch: char| !ch.is_whitespace();
        let (mut a, mut b) = (c.ch.min(text.len()), c.ch.min(text.len()));
        while a > 0 && word(text[a - 1]) {
            a -= 1;
        }
        while b < text.len() && word(text[b]) {
            b += 1;
        }
        self.anchor = Some(Cursor { ch: a, ..c });
        self.cursor = Cursor { ch: b, ..c };
        self.hist.end(&self.document);
    }

    /// Selects the whole paragraph (or cell) under the caret.
    pub fn select_item(&mut self) {
        let c = self.cursor;
        let len = self.item_len(c.block, c.cell).unwrap_or(0);
        self.anchor = Some(Cursor { ch: 0, ..c });
        self.cursor = Cursor { ch: len, ..c };
        self.hist.end(&self.document);
    }
}

#[cfg(test)]
mod tests;
