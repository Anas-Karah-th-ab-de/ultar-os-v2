//! Formatting operations of [`Doc`]: character and paragraph formatting, lists, page break,
//! pictures and the blocks of the app's own rich clipboard.

use crate::state::{Cursor, Doc};
use ultar_office::doc::{Align, Block, Dir, Para, Picture, Run, Style};

impl Doc {
    // ---- character formatting ---------------------------------------------

    /// The look at the caret: what typing there would produce.
    pub fn look_at_caret(&self) -> Run {
        if let Some(look) = &self.look {
            return look.clone();
        }
        let c = self.cursor;
        match self.document.blocks.get(c.block) {
            Some(Block::Para(p)) if c.cell.is_none() => p.format_at(c.ch),
            _ => Run::default(),
        }
    }

    /// The style of the paragraph the caret is in.
    pub fn style_at_caret(&self) -> Style {
        match self.document.blocks.get(self.cursor.block) {
            Some(Block::Para(p)) if self.cursor.cell.is_none() => p.style,
            _ => Style::Normal,
        }
    }

    fn para_ranges(&self) -> Vec<(usize, usize, usize)> {
        let Some((a, b)) = self.selection() else {
            return Vec::new();
        };
        (a.block..=b.block)
            .filter_map(|bi| match self.document.blocks.get(bi) {
                Some(Block::Para(p)) => {
                    let from = if bi == a.block { a.ch } else { 0 };
                    let to = if bi == b.block { b.ch } else { p.char_len() };
                    Some((bi, from, to))
                }
                _ => None,
            })
            .collect()
    }

    /// Changes the look of the selected text with `f` (given the paragraph's style); without
    /// a selection the look of the text typed next.
    pub fn apply_run(&mut self, f: &dyn Fn(&mut Run, Style)) {
        let ranges = self.para_ranges();
        if self.selection().is_none() {
            let style = self.style_at_caret();
            let mut look = self.look_at_caret();
            f(&mut look, style);
            self.look = Some(look);
            return;
        }
        self.begin();
        for (bi, from, to) in ranges {
            if let Some(Block::Para(p)) = self.document.blocks.get_mut(bi) {
                let style = p.style;
                p.format_range(from, to, &|r| f(r, style));
            }
        }
        self.hist.end(&self.document);
    }

    /// `true` when `get` holds for all selected text (for the caret's look without selection).
    pub fn all_selected(&self, get: &dyn Fn(&Run) -> bool) -> bool {
        if self.selection().is_none() {
            return get(&self.look_at_caret());
        }
        self.para_ranges()
            .into_iter()
            .all(|(bi, from, to)| match self.document.blocks.get(bi) {
                Some(Block::Para(p)) => p.all_in_range(from, to, get),
                _ => true,
            })
    }

    /// Toggles a yes/no property like Word: on unless all of the selection has it already.
    pub fn toggle_run(&mut self, get: fn(&Run) -> bool, set: fn(&mut Run, bool)) {
        let on = !self.all_selected(&get);
        self.apply_run(&move |r, _| set(r, on));
    }

    /// Makes the selected text (or the next typed text) `delta` points bigger or smaller.
    pub fn change_size(&mut self, delta: f32) {
        self.apply_run(&move |r, style| {
            let now = r.size_pt.unwrap_or_else(|| style.size());
            r.size_pt = Some((now + delta).clamp(6.0, 96.0));
        });
    }

    // ---- paragraph formatting -----------------------------------------------

    fn for_each_para(&mut self, f: &dyn Fn(&mut Para)) {
        let (a, b) = self.selection().unwrap_or((self.cursor, self.cursor));
        self.begin();
        for bi in a.block..=b.block {
            if let Some(Block::Para(p)) = self.document.blocks.get_mut(bi) {
                f(p);
            }
        }
        self.hist.end(&self.document);
    }

    pub fn set_style(&mut self, style: Style) {
        self.for_each_para(&|p| p.style = style);
    }

    pub fn set_align(&mut self, align: Align) {
        self.for_each_para(&|p| p.align = align);
    }

    pub fn set_dir(&mut self, dir: Dir) {
        self.for_each_para(&|p| p.dir = dir);
    }

    pub fn set_line_spacing(&mut self, spacing: Option<f32>) {
        self.for_each_para(&|p| p.line_spacing = spacing);
    }

    /// Moves the paragraph's start indent by `delta_pt` (never below zero).
    pub fn indent(&mut self, delta_pt: f32) {
        self.for_each_para(&|p| {
            let next = (p.indent_pt.unwrap_or(0.0) + delta_pt).clamp(0.0, 400.0);
            p.indent_pt = (next > 0.0).then_some(next);
        });
    }

    /// Bullet or numbered list on/off for the paragraphs of the selection.
    pub fn toggle_list(&mut self, bullet: bool) {
        let want = if bullet { Style::Bullet } else { Style::Number };
        self.for_each_para(&|p| p.style = if p.style == want { Style::Normal } else { want });
    }

    /// A page break at the caret: the text after it starts the next page. At the start of a
    /// paragraph it switches the break before that paragraph on or off.
    pub fn insert_page_break(&mut self) {
        let c = self.cursor;
        if c.cell.is_some() {
            return;
        }
        self.begin();
        if let Some(Block::Para(p)) = self.document.blocks.get_mut(c.block) {
            if c.ch == 0 {
                p.page_break = !p.page_break;
            } else {
                let mut tail = p.split_off(c.ch);
                tail.page_break = true;
                self.document.blocks.insert(c.block + 1, Block::Para(tail));
                self.cursor = Cursor {
                    block: c.block + 1,
                    cell: None,
                    ch: 0,
                };
            }
        }
        self.hist.end(&self.document);
    }

    // ---- pictures ---------------------------------------------------------

    /// Puts a picture after the caret's block and the caret into a paragraph below it.
    pub fn insert_picture(&mut self, pic: Picture) {
        self.begin();
        let at = self.cursor.block + 1;
        self.document.blocks.insert(at, Block::Picture(pic));
        if !matches!(self.document.blocks.get(at + 1), Some(Block::Para(_))) {
            self.document
                .blocks
                .insert(at + 1, Block::Para(Para::default()));
        }
        self.cursor = Cursor {
            block: at + 1,
            cell: None,
            ch: 0,
        };
        self.anchor = None;
        self.hist.end(&self.document);
        self.clamp();
    }

    /// Makes the picture at the caret `factor` times as large (a picture block must be at
    /// the caret). Returns whether one was.
    pub fn resize_picture(&mut self, factor: f32) -> bool {
        let at = self.cursor.block;
        if !matches!(self.document.blocks.get(at), Some(Block::Picture(_))) {
            return false;
        }
        self.begin();
        if let Some(Block::Picture(p)) = self.document.blocks.get_mut(at) {
            let (w, h) = (p.width_pt.unwrap_or(200.0), p.height_pt.unwrap_or(150.0));
            p.width_pt = Some((w * factor).clamp(16.0, 2000.0));
            p.height_pt = Some((h * factor).clamp(16.0, 2000.0));
        }
        self.hist.end(&self.document);
        true
    }

    // ---- the app's own rich clipboard ----------------------------------------

    /// The selected paragraphs with their formatting (tables and pictures as a whole).
    pub fn copy_blocks(&self) -> Vec<Block> {
        let Some((a, b)) = self.selection() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for bi in a.block..=b.block {
            match self.document.blocks.get(bi) {
                Some(Block::Para(p)) => {
                    let from = if bi == a.block { a.ch } else { 0 };
                    let to = if bi == b.block { b.ch } else { p.char_len() };
                    out.push(Block::Para(p.sub_para(from, to)));
                }
                Some(other) if a.block != b.block => out.push(other.clone()),
                _ => {}
            }
        }
        out
    }

    /// Pastes the text of the paragraphs of `blocks` (inside a table cell, or next to a picture).
    fn paste_as_text(&mut self, blocks: &[Block]) {
        let text: Vec<String> = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Para(p) => Some(p.text()),
                _ => None,
            })
            .collect();
        self.insert(&text.join("\n"));
    }

    /// Pastes blocks from [`Doc::copy_blocks`] at the caret: a single paragraph goes inline
    /// with its formatting, several split the paragraph.
    pub fn paste_blocks(&mut self, blocks: Vec<Block>) {
        if blocks.is_empty() {
            return;
        }
        if self.selection().is_some() {
            self.delete_selection();
        }
        let c = self.cursor;
        if c.cell.is_some() || !matches!(self.document.blocks.get(c.block), Some(Block::Para(_))) {
            return self.paste_as_text(&blocks);
        }
        self.begin();
        let Some(Block::Para(host)) = self.document.blocks.get_mut(c.block) else {
            return;
        };
        let mut tail = host.split_off(c.ch);
        let mut at = c.block;
        let last = blocks.len() - 1;
        let mut end_ch = c.ch;
        for (i, b) in blocks.into_iter().enumerate() {
            match b {
                Block::Para(p) if i == 0 => {
                    if let Some(Block::Para(h)) = self.document.blocks.get_mut(at) {
                        end_ch = h.char_len() + p.char_len();
                        h.append(&p);
                        if i < last {
                            h.style = p.style;
                        }
                    }
                }
                Block::Para(p) => {
                    end_ch = p.char_len();
                    self.document.blocks.insert(at + 1, Block::Para(p));
                    at += 1;
                }
                other => {
                    self.document.blocks.insert(at + 1, other);
                    at += 1;
                    end_ch = 0;
                }
            }
        }
        match self.document.blocks.get_mut(at) {
            Some(Block::Para(last_para)) => last_para.append(&std::mem::take(&mut tail)),
            _ => self
                .document
                .blocks
                .insert(at + 1, Block::Para(std::mem::take(&mut tail))),
        }
        self.cursor = Cursor {
            block: at,
            cell: None,
            ch: end_ch,
        };
        self.hist.end(&self.document);
        self.clamp();
    }
}
