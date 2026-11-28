//! The open word document: the `ultar-office` model, a caret and a selection that live in
//! character space, an undo stack of full snapshots (one per typing burst), and the flat,
//! wrapped line index the text view draws. Editing goes through small run-level helpers, so
//! mixed Arabic/Latin formatting inside a paragraph survives typing.

use std::path::{Path, PathBuf};
use ultar_office::doc::{Align, Block, Dir, Document, Para, Run, Style};
use ultar_textview::Source;

/// Largest file the app opens.
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;
/// Longest wrapped line, characters.
pub const MIN_CHARS: usize = 8;

/// A cell of a table block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Cell {
    pub row: usize,
    pub col: usize,
}

/// A position: a block and a character offset inside it (`ch` counts characters, not bytes).
/// `cell` addresses one cell of a table block; `None` means the paragraph itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Cursor {
    pub block: usize,
    pub cell: Option<Cell>,
    pub ch: usize,
}

/// One visual line of the flat view.
#[derive(Clone, Debug)]
pub struct FlatLine {
    pub block: usize,
    pub cell: Option<Cell>,
    /// Character offset of the line's first character in the item's text.
    pub start: usize,
    pub text: String,
    pub rtl: bool,
}

/// The wrapped lines the text view shows.
pub struct FlatView {
    pub lines: Vec<FlatLine>,
}

impl Source for FlatView {
    fn lines(&self) -> usize {
        self.lines.len()
    }
    fn line(&self, i: usize) -> &str {
        self.lines.get(i).map_or("", |l| l.text.as_str())
    }
}

/// One undo step: the document and the caret before an edit.
#[derive(Clone)]
struct Step {
    document: Document,
    cursor: Cursor,
    anchor: Option<Cursor>,
}

/// A word processor document with its file, caret, selection and undo history.
pub struct Doc {
    pub document: Document,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub cursor: Cursor,
    pub anchor: Option<Cursor>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    burst: bool,
    /// Zoom factor 0.5 ..= 3.0 (fewer/more characters per line).
    pub zoom: f32,
    /// A page (A4/Letter with margins) or a flowing phone view.
    pub page_view: bool,
}

/// Why a document could not be opened or saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocError {
    Denied,
    Missing,
    Exists,
    BadName,
    TooBig,
    Unsupported,
    Io,
}

impl DocError {
    pub fn key(self) -> &'static str {
        match self {
            DocError::Denied => "doc.reason.denied",
            DocError::Missing => "doc.reason.missing",
            DocError::Exists => "doc.reason.exists",
            DocError::BadName => "doc.reason.name",
            DocError::TooBig => "doc.reason.too_big",
            DocError::Unsupported => "doc.reason.unsupported",
            DocError::Io => "doc.reason.io",
        }
    }
    fn of(e: &std::io::Error) -> DocError {
        match e.kind() {
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => {
                DocError::Denied
            }
            std::io::ErrorKind::NotFound => DocError::Missing,
            std::io::ErrorKind::AlreadyExists => DocError::Exists,
            _ => DocError::Io,
        }
    }
}

impl Default for Doc {
    fn default() -> Doc {
        Doc::new()
    }
}

impl Doc {
    /// An empty document with one empty paragraph.
    pub fn new() -> Doc {
        let mut document = Document::default();
        document.blocks.push(Block::Para(Para::default()));
        Doc {
            document,
            path: None,
            dirty: false,
            cursor: Cursor::default(),
            anchor: None,
            undo: Vec::new(),
            redo: Vec::new(),
            burst: false,
            zoom: 1.0,
            page_view: true,
        }
    }

    /// Opens a `.docx` or a plain-text file (`.odt` and other formats are refused).
    pub fn open(path: &Path) -> Result<Doc, DocError> {
        let size = std::fs::metadata(path).map_err(|e| DocError::of(&e))?.len();
        if size > MAX_BYTES {
            return Err(DocError::TooBig);
        }
        let bytes = std::fs::read(path).map_err(|e| DocError::of(&e))?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let document = match ext.as_str() {
            "docx" | "docm" | "dotx" => {
                ultar_office::docx::read(&bytes).map_err(|_| DocError::Unsupported)?
            }
            "md" | "markdown" => {
                let t = ultar_office::code::decode(&bytes).0;
                ultar_office::doc::Document::from_markdown(&t)
            }
            "txt" | "text" => {
                let t = ultar_office::code::decode(&bytes).0;
                ultar_office::doc::Document::from_text(&t)
            }
            _ => return Err(DocError::Unsupported),
        };
        let mut d = Doc {
            document,
            path: Some(path.to_path_buf()),
            dirty: false,
            zoom: 1.0,
            page_view: true,
            ..Doc::new()
        };
        d.clamp();
        Ok(d)
    }

    /// The shown name (`None`: unsaved).
    pub fn name(&self) -> Option<String> {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
    }

    /// `true` when there are unsaved changes.
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// Saves to its file (always `.docx`; a `save as` must choose a `.docx` name).
    pub fn save(&mut self) -> Result<(), DocError> {
        let path = self.path.clone().ok_or(DocError::BadName)?;
        write_doc(&path, &self.document)?;
        self.dirty = false;
        Ok(())
    }

    /// Saves under a new name (must not exist yet).
    pub fn save_as(&mut self, path: &Path) -> Result<(), DocError> {
        if path.exists() {
            return Err(DocError::Exists);
        }
        write_doc(path, &self.document)?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        Ok(())
    }

    /// The text of the item the caret is in.
    pub fn item_text(&self, block: usize, cell: Option<Cell>) -> Option<String> {
        self.document.blocks.get(block).and_then(|b| match (b, cell) {
            (Block::Para(p), None) => Some(p.text()),
            (Block::Table(t), Some(c)) => t.get(c.row).and_then(|r| r.get(c.col)).cloned(),
            _ => None,
        })
    }

    fn item_len(&self, block: usize, cell: Option<Cell>) -> Option<usize> {
        self.item_text(block, cell).map(|t| t.chars().count())
    }

    /// Clamps the caret (and ensures the document has at least one editable block).
    pub fn clamp(&mut self) {
        if self.document.blocks.is_empty() {
            self.document.blocks.push(Block::Para(Para::default()));
        }
        let mut c = self.cursor;
        c.block = c.block.min(self.document.blocks.len() - 1);
        match self.document.blocks.get(c.block) {
            Some(Block::Para(_)) => {
                c.cell = None;
            }
            Some(Block::Table(t)) => {
                let cell = c.cell.unwrap_or(Cell { row: 0, col: 0 });
                let row = cell.row.min(t.len().saturating_sub(1));
                let col = cell
                    .col
                    .min(t.get(row).map_or(0, |r| r.len().saturating_sub(1)));
                c.cell = Some(Cell { row, col });
            }
            _ => {
                c.block = 0;
                c.ch = 0;
                c.cell = None;
            }
        }
        if let Some(len) = self.item_len(c.block, c.cell) {
            c.ch = c.ch.min(len);
        }
        self.cursor = c;
    }

    // ---- selection --------------------------------------------------------

    /// The selection as (start, end) in document order; `None` without selection.
    pub fn selection(&self) -> Option<(Cursor, Cursor)> {
        let b = self.anchor?;
        Some(if self.cmp(self.cursor, b).is_le() {
            (self.cursor, b)
        } else {
            (b, self.cursor)
        })
    }

    fn cmp(&self, a: Cursor, b: Cursor) -> std::cmp::Ordering {
        (a.block, a.cell, a.ch).cmp(&(b.block, b.cell, b.ch))
    }

    /// The selected text (or the empty string).
    pub fn selected_text(&self) -> String {
        let Some((a, b)) = self.selection() else {
            return String::new();
        };
        if a.block == b.block && a.cell == b.cell {
            return self
                .item_text(a.block, a.cell)
                .map(|t| chars(&t, a.ch, b.ch))
                .unwrap_or_default();
        }
        let mut parts = Vec::new();
        for bi in a.block..=b.block {
            if let Some(t) = self.item_text(bi, None) {
                let from = if bi == a.block { a.ch } else { 0 };
                let to = if bi == b.block {
                    b.ch.min(t.chars().count())
                } else {
                    t.chars().count()
                };
                let s = chars(&t, from, to);
                if !s.is_empty() || bi == a.block || bi == b.block {
                    parts.push(s);
                }
            }
        }
        parts.join("\n")
    }

    /// Removes the selection and returns whether anything was removed.
    pub fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            return false;
        };
        self.begin();
        if a.block == b.block && a.cell == b.cell {
            self.delete_span(a.block, a.cell, a.ch, b.ch);
        } else if a.cell.is_none() && b.cell.is_none() {
            // Paragraphs: collapse the range into one paragraph (head of the first
            // plus tail of the last).
            let head = self
                .item_text(a.block, None)
                .map(|t| chars(&t, 0, a.ch))
                .unwrap_or_default();
            let tail = self
                .item_text(b.block, None)
                .map(|t| chars(&t, b.ch, t.chars().count()))
                .unwrap_or_default();
            let style = match self.document.blocks.get(a.block) {
                Some(Block::Para(p)) => p.style,
                _ => Style::Normal,
            };
            let merged = head + &tail;
            for _ in a.block..=b.block {
                let _ = self.document.blocks.remove(a.block);
            }
            self.document
                .blocks
                .insert(a.block, Block::Para(Para::new(style, &merged)));
            self.cursor = Cursor { block: a.block, cell: None, ch: a.ch };
        }
        self.anchor = None;
        self.stop_and_mark();
        true
    }

    // ---- small edits ------------------------------------------------------

    /// Inserts text at the caret (replacing a selection); newlines split paragraphs.
    pub fn insert(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        if self.selection().is_some() {
            self.delete_selection();
        }
        for (i, part) in s.split('\n').enumerate() {
            if i > 0 {
                self.newline();
            }
            if !part.is_empty() {
                self.insert_plain(part);
            }
        }
    }

    /// The raw insertion of one piece of text without a newline.
    fn insert_plain(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.begin();
        let c = self.cursor;
        self.with_item_mut(c.block, c.cell, |text| {
            let at = c.ch.min(text.chars().count());
            let mut t = text.clone();
            t.insert_str(char_byte(&t, at), s);
            *text = t;
        });
        self.cursor.ch += s.chars().count();
        self.dirty = true;
        self.stop_and_mark();
    }

    /// Backspace: deletes the character before the caret or merges paragraphs.
    pub fn backspace(&mut self) {
        if self.selection().is_some() {
            self.delete_selection();
            return;
        }
        self.begin();
        let c = self.cursor;
        if c.ch > 0 {
            self.delete_span(c.block, c.cell, c.ch - 1, c.ch);
            self.cursor.ch -= 1;
        } else if c.cell.is_none() && c.block > 0 {
            let this = self.item_text(c.block, None).unwrap_or_default();
            let plen = self.item_len(c.block - 1, None).unwrap_or(0);
            self.with_item_mut(c.block - 1, None, |text| {
                text.push_str(&this);
            });
            self.document.blocks.remove(c.block);
            self.cursor = Cursor { block: c.block - 1, cell: None, ch: plen };
        } else {
            self.stop_and_mark();
            return;
        }
        self.dirty = true;
        self.stop_and_mark();
    }

    /// Delete: removes the character after the caret.
    pub fn delete(&mut self) {
        if self.selection().is_some() {
            self.delete_selection();
            return;
        }
        self.begin();
        let c = self.cursor;
        let len = self.item_len(c.block, c.cell).unwrap_or(0);
        if c.ch < len {
            self.delete_span(c.block, c.cell, c.ch, c.ch + 1);
            self.dirty = true;
        }
        self.stop_and_mark();
    }

    /// Enter: splits the paragraph at the caret (in a table cell it stays one cell).
    pub fn newline(&mut self) {
        if self.selection().is_some() {
            self.delete_selection();
        }
        self.begin();
        let c = self.cursor;
        if c.cell.is_none() {
            if let Some(Block::Para(p)) = self.document.blocks.get_mut(c.block) {
                let at = c.ch.min(p.text().chars().count());
                let rest = split_para(p, at);
                self.document.blocks.insert(c.block + 1, Block::Para(rest));
                self.cursor = Cursor { block: c.block + 1, cell: None, ch: 0 };
                self.dirty = true;
            }
        } else {
            self.insert_plain("\n");
            return;
        }
        self.stop_and_mark();
    }

    /// Removes `[from, to)` characters of one item.
    fn delete_span(&mut self, block: usize, cell: Option<Cell>, from: usize, to: usize) {
        if from >= to {
            return;
        }
        match (self.document.blocks.get_mut(block), cell) {
            (Some(Block::Para(p)), None) => {
                let text = p.text();
                let kept = chars(&text, 0, from.min(text.chars().count()))
                    + &chars(&text, to.min(text.chars().count()), text.chars().count());
                p.set_text(&kept);
            }
            (Some(Block::Table(t)), Some(c)) => {
                if let Some(s) = t.get_mut(c.row).and_then(|r| r.get_mut(c.col)) {
                    let a = char_byte(s, from.min(s.chars().count()));
                    let b = char_byte(s, to.min(s.chars().count()));
                    s.replace_range(a..b, "");
                }
            }
            _ => {}
        }
    }

    fn with_item_mut(&mut self, block: usize, cell: Option<Cell>, f: impl FnOnce(&mut String)) {
        match (self.document.blocks.get_mut(block), cell) {
            (Some(Block::Para(p)), None) => {
                let mut t = p.text();
                f(&mut t);
                p.set_text(&t);
            }
            (Some(Block::Table(t)), Some(c)) => {
                if let Some(s) = t.get_mut(c.row).and_then(|r| r.get_mut(c.col)) {
                    f(s);
                }
            }
            _ => {}
        }
    }

    // ---- undo ------------------------------------------------------------

    /// Opens an undo step before the first change of a burst.
    fn begin(&mut self) {
        if !self.burst {
            self.undo.push(Step {
                document: self.document.clone(),
                cursor: self.cursor,
                anchor: self.anchor,
            });
            self.redo.clear();
            self.burst = true;
        }
    }

    /// Ends a typing burst (movement, formatting or other commands call this).
    pub fn burst_break(&mut self) {
        self.burst = false;
    }

    fn stop_and_mark(&mut self) {
        self.burst_break();
        self.clamp();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        self.burst_break();
        let Some(prev) = self.undo.pop() else {
            return false;
        };
        let now = Step {
            document: std::mem::replace(&mut self.document, prev.document),
            cursor: self.cursor,
            anchor: self.anchor,
        };
        self.cursor = prev.cursor;
        self.anchor = prev.anchor;
        self.redo.push(now);
        self.dirty = true;
        self.clamp();
        true
    }

    pub fn redo(&mut self) -> bool {
        self.burst_break();
        let Some(next) = self.redo.pop() else {
            return false;
        };
        let now = Step {
            document: std::mem::replace(&mut self.document, next.document),
            cursor: self.cursor,
            anchor: self.anchor,
        };
        self.cursor = next.cursor;
        self.anchor = next.anchor;
        self.undo.push(now);
        self.dirty = true;
        self.clamp();
        true
    }

    // ---- movement ---------------------------------------------------------

    pub fn move_chars(&mut self, delta: isize, extend: bool) {
        let c = self.cursor;
        let len = self.item_len(c.block, c.cell).unwrap_or(0) as isize;
        let ch = (c.ch as isize + delta).clamp(0, len) as usize;
        self.set_pos(Cursor { ch, ..c }, extend);
    }

    pub fn home(&mut self, extend: bool) {
        let c = self.cursor;
        self.set_pos(Cursor { ch: 0, ..c }, extend);
    }

    pub fn end(&mut self, extend: bool) {
        let c = self.cursor;
        let len = self.item_len(c.block, c.cell).unwrap_or(0);
        self.set_pos(Cursor { ch: len, ..c }, extend);
    }

    pub fn move_block(&mut self, delta: isize, extend: bool) {
        let b = (self.cursor.block as isize + delta).clamp(0, self.document.blocks.len() as isize - 1) as usize;
        let cell = if matches!(self.document.blocks.get(b), Some(Block::Table(_))) {
            Some(Cell { row: 0, col: 0 })
        } else {
            None
        };
        self.set_pos(Cursor { block: b, cell, ch: 0 }, extend);
    }

    pub fn select_all(&mut self) {
        self.cursor = Cursor::default();
        self.anchor = None;
        let last = self.document.blocks.len().saturating_sub(1);
        let len = self.item_len(last, None).unwrap_or(0);
        self.anchor = Some(Cursor { block: last, cell: None, ch: len });
        self.burst_break();
    }

    fn set_pos(&mut self, c: Cursor, extend: bool) {
        if !extend {
            self.anchor = None;
        } else if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
        self.cursor = c;
        self.burst_break();
        self.clamp();
    }

    // ---- formatting (paragraph level) ------------------------------------

    fn for_each_para(&mut self, f: impl Fn(&mut Para)) {
        let (a, b) = self.selection().unwrap_or((self.cursor, self.cursor));
        for bi in a.block..=b.block {
            if let Some(Block::Para(p)) = self.document.blocks.get_mut(bi) {
                f(p);
            }
        }
        self.dirty = true;
    }

    pub fn set_style(&mut self, style: Style) {
        self.for_each_para(|p| p.style = style);
    }

    pub fn set_align(&mut self, align: Align) {
        self.for_each_para(|p| p.align = align);
    }

    pub fn set_dir(&mut self, dir: Dir) {
        self.for_each_para(|p| p.dir = dir);
    }

    pub fn set_line_spacing(&mut self, sp: Option<f32>) {
        self.for_each_para(|p| p.line_spacing = sp);
    }

    pub fn indent(&mut self, delta_pt: f32) {
        self.for_each_para(|p| {
            let cur = p.indent_pt.unwrap_or(0.0);
            let next = (cur + delta_pt).clamp(0.0, 600.0);
            p.indent_pt = (next > 0.0).then_some(next);
        });
    }

    pub fn toggle_list(&mut self, bullet: bool) {
        self.for_each_para(|p| {
            p.style = if p.style == (if bullet { Style::Bullet } else { Style::Number }) {
                Style::Normal
            } else if bullet {
                Style::Bullet
            } else {
                Style::Number
            };
        });
    }

    // ---- formatting (run level) ------------------------------------------

    /// Applies `f` to every run that touches the selection (or the caret's run).
    pub fn for_each_run(&mut self, f: impl Fn(&mut Run)) {
        let (a, b) = self.selection().unwrap_or_else(|| {
            let end = Cursor { ch: self.cursor.ch.saturating_add(1), ..self.cursor };
            (self.cursor, end)
        });
        for bi in a.block..=b.block {
            let from = if bi == a.block { a.ch } else { 0 };
            let len = self.item_len(bi, None).unwrap_or(0);
            let to = if bi == b.block { b.ch } else { len };
            if let Some(Block::Para(p)) = self.document.blocks.get_mut(bi) {
                apply_runs(p, from, to, &f);
            }
        }
        self.dirty = true;
    }

    // ---- flattening -------------------------------------------------------

    /// Builds the wrapped line view for `max_chars` characters per line.
    pub fn flatten(&self, max_chars: usize) -> FlatView {
        let max = max_chars.max(MIN_CHARS);
        let mut lines = Vec::new();
        for (bi, b) in self.document.blocks.iter().enumerate() {
            match b {
                Block::Para(p) => {
                    for (start, text) in wrap(p.text(), max) {
                        lines.push(FlatLine { block: bi, cell: None, start, rtl: p.rtl(), text });
                    }
                }
                Block::Table(t) => {
                    for (ri, row) in t.iter().enumerate() {
                        for (ci, cell) in row.iter().enumerate() {
                            for (start, text) in wrap(cell.clone(), max) {
                                lines.push(FlatLine {
                                    block: bi,
                                    cell: Some(Cell { row: ri, col: ci }),
                                    start,
                                    rtl: false,
                                    text,
                                });
                            }
                        }
                    }
                }
                Block::Picture(_) => {
                    lines.push(FlatLine {
                        block: bi,
                        cell: None,
                        start: 0,
                        rtl: false,
                        text: String::new(),
                    });
                }
            }
        }
        if lines.is_empty() {
            lines.push(FlatLine { block: 0, cell: None, start: 0, rtl: false, text: String::new() });
        }
        FlatView { lines }
    }

    /// Maps a flat line + byte to a document cursor.
    pub fn cursor_at(&self, view: &FlatView, line: usize, byte: usize) -> Cursor {
        let Some(l) = view.lines.get(line) else {
            return self.cursor;
        };
        let before = l.text.get(..byte.min(l.text.len())).map_or(0, |s| s.chars().count());
        Cursor { block: l.block, cell: l.cell, ch: l.start + before }
    }

    /// The byte offset of a cursor inside its flat line; `None` when the cursor is elsewhere.
    pub fn byte_in(&self, view: &FlatView, c: Cursor) -> Option<usize> {
        view.lines.iter().find_map(|l| {
            if l.block == c.block
                && l.cell == c.cell
                && c.ch >= l.start
                && c.ch <= l.start + l.text.chars().count()
            {
                Some(char_byte(&l.text, c.ch - l.start))
            } else {
                None
            }
        })
    }
}

// ---- pure helpers --------------------------------------------------------

/// Wraps `text` at word boundaries into chunks of at most `max` characters; returns
/// `(character offset, chunk)` pairs.
pub fn wrap(text: String, max: usize) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut line_start = 0usize;
    let mut line_chars = 0usize;
    let mut line = String::new();
    for word in text.split_inclusive([' ', '\t']) {
        if word.is_empty() {
            continue;
        }
        let wc = word.chars().count();
        // a trailing separator does not count against the line width
        let measure = if word.ends_with([' ', '\t']) { wc.saturating_sub(1) } else { wc };
        if !line.is_empty() && line_chars + measure > max {
            out.push((line_start, std::mem::take(&mut line)));
            line_start = pos;
            line_chars = 0;
        }
        if line.is_empty() {
            line_start = pos;
        }
        line.push_str(word);
        line_chars += measure;
        pos += wc;
    }
    if !line.is_empty() {
        out.push((line_start, line));
    } else if text.is_empty() {
        out.push((0, String::new()));
    }
    out
}

fn char_byte(s: &str, ch: usize) -> usize {
    s.char_indices().nth(ch).map_or(s.len(), |(i, _)| i)
}

fn chars(s: &str, from: usize, to: usize) -> String {
    s.chars().skip(from).take(to.saturating_sub(from)).collect()
}

/// Splits a paragraph at character `at`: the rest is returned as a new run set with the
/// formatting of the run at the split point; the first part replaces `p`'s runs in the caller.
fn split_para(p: &mut Para, at: usize) -> Para {
    let rest = Para {
        style: p.style,
        align: p.align,
        dir: p.dir,
        line_spacing: p.line_spacing,
        indent_pt: p.indent_pt,
        runs: Vec::new(),
    };
    let right = take_right_runs(p, at);
    let left = take_left_runs(&mut p.runs, at);
    p.runs = left;
    Para { runs: right, ..rest }
}

fn take_right_runs(p: &Para, at: usize) -> Vec<Run> {
    let mut out = Vec::new();
    let mut acc = 0usize;
    for r in &p.runs {
        let rc = r.text.chars().count();
        let start = at.saturating_sub(acc).min(rc);
        if at >= acc + rc {
            // wholly before: nothing
        } else if at <= acc {
            out.push(r.clone());
        } else if start < rc {
            let t = chars(&r.text, start, rc);
            if !t.is_empty() {
                out.push(Run { text: t, ..r.clone() });
            }
        }
        acc += rc;
    }
    if out.is_empty() {
        out.push(Run::default());
    }
    out
}

fn take_left_runs(runs: &mut Vec<Run>, at: usize) -> Vec<Run> {
    let mut out = Vec::new();
    let mut acc = 0usize;
    let mut take = true;
    for r in runs.drain(..) {
        let rc = r.text.chars().count();
        if !take {
            continue;
        }
        if acc + rc <= at {
            out.push(r);
        } else {
            let cut = at - acc;
            let l = chars(&r.text, 0, cut);
            if !l.is_empty() {
                out.push(Run { text: l, ..r });
            }
            take = false;
        }
        acc += rc;
    }
    if out.is_empty() {
        out.push(Run::default());
    }
    out
}

/// Applies `f` to the runs overlapping `[from, to)` by splitting at the edges first.
fn apply_runs(p: &mut Para, from: usize, to: usize, f: &impl Fn(&mut Run)) {
    if from >= to {
        return;
    }
    let mut out: Vec<Run> = Vec::new();
    let mut acc = 0usize;
    for r in &p.runs {
        let rc = r.text.chars().count();
        let s = acc.max(from).min(acc + rc);
        let e = (acc + rc).min(to).max(acc);
        if acc < s {
            let left = chars(&r.text, 0, s - acc);
            if !left.is_empty() {
                out.push(Run { text: left, ..r.clone() });
            }
        }
        if s < e {
            let mut m = Run { text: chars(&r.text, s - acc, e - acc), ..r.clone() };
            f(&mut m);
            if !m.text.is_empty() {
                out.push(m);
            }
        }
        if e < acc + rc {
            let right = chars(&r.text, e - acc, rc);
            if !right.is_empty() {
                out.push(Run { text: right, ..r.clone() });
            }
        }
        acc += rc;
    }
    if out.is_empty() {
        out.push(Run::default());
    }
    p.runs = out;
}

// ---- file I/O ------------------------------------------------------------

fn write_doc(path: &Path, d: &Document) -> Result<(), DocError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext != "docx" {
        return Err(DocError::Unsupported);
    }
    let bytes = ultar_office::docx::write(d).map_err(|_| DocError::Io)?;
    write_replacing(path, &bytes)
}

fn write_replacing(path: &Path, bytes: &[u8]) -> Result<(), DocError> {
    let name = path
        .file_name()
        .ok_or(DocError::BadName)?
        .to_string_lossy()
        .into_owned();
    let part = path.with_file_name(format!(".{name}.part"));
    let written = std::fs::File::create(&part).and_then(|mut f| {
        use std::io::Write;
        f.write_all(bytes)?;
        f.sync_all()
    });
    if let Err(e) = written {
        let _ = std::fs::remove_file(&part);
        return Err(DocError::of(&e));
    }
    std::fs::rename(&part, path).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        DocError::of(&e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(doc: &mut Doc, s: &str) {
        for c in s.chars() {
            if c == '\n' {
                doc.newline();
            } else {
                doc.insert(&c.to_string());
            }
        }
    }

    #[test]
    fn typing_splits_paragraphs() {
        let mut d = Doc::new();
        typed(&mut d, "سطر أول");
        d.newline();
        typed(&mut d, "second");
        assert_eq!(d.document.blocks.len(), 2);
        assert_eq!(d.item_text(0, None), Some("سطر أول".into()));
        assert_eq!(d.item_text(1, None), Some("second".into()));
    }

    #[test]
    fn backspace_merges_paragraphs() {
        let mut d = Doc::new();
        typed(&mut d, "a");
        d.newline();
        assert_eq!(d.document.blocks.len(), 2);
        d.backspace(); // caret at the start of the empty second paragraph
        assert_eq!(d.document.blocks.len(), 1);
        assert_eq!(d.item_text(0, None), Some("a".into()));
    }

    #[test]
    fn formatting_applies_to_the_selection() {
        let mut d = Doc::new();
        typed(&mut d, "abc");
        d.home(false);
        d.anchor = Some(Cursor { block: 0, cell: None, ch: 3 });
        d.for_each_run(|r| r.bold = true);
        let Some(Block::Para(p)) = d.document.blocks.first() else {
            panic!("missing para");
        };
        assert!(p.runs.iter().any(|r| r.bold), "selection is bold");
        assert_eq!(p.text(), "abc");
    }

    #[test]
    fn undo_restores_typing() {
        let mut d = Doc::new();
        typed(&mut d, "a");
        d.burst_break();
        typed(&mut d, "b");
        assert_eq!(d.item_text(0, None), Some("ab".into()));
        assert!(d.undo());
        assert_eq!(d.item_text(0, None), Some("a".into()));
        assert!(d.redo());
        assert_eq!(d.item_text(0, None), Some("ab".into()));
    }

    #[test]
    fn wrap_breaks_at_words() {
        let lines = wrap("one two three".into(), 7);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].0, 0);
        assert!(lines[0].1.trim_end() == "one two");
        assert_eq!(lines[1].0, 8);
        assert_eq!(lines[1].1, "three");
    }

    #[test]
    fn table_cells_are_editable() {
        let mut d = Doc::new();
        d.document.blocks.push(Block::Table(vec![vec!["a".into(), "b".into()]]));
        d.cursor = Cursor { block: 1, cell: Some(Cell { row: 0, col: 0 }), ch: 1 };
        d.insert("x");
        assert_eq!(
            d.item_text(1, Some(Cell { row: 0, col: 0 })),
            Some("ax".into())
        );
    }
}