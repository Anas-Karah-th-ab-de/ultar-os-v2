//! The open word document: the `ultar-office` model, a caret and a selection that live in
//! character space (block, optional table cell, character), the undo history and the file it
//! came from. The editing operations are in `edit.rs`, tables in `tables.rs`.

use crate::history::History;
use std::path::{Path, PathBuf};
use ultar_office::doc::{Block, Document, Para, Run};

/// Largest file the app opens.
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;
/// Smallest and largest zoom.
pub const ZOOM_MIN: f32 = 0.5;
pub const ZOOM_MAX: f32 = 3.0;

/// A cell of a table block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Cell {
    pub row: usize,
    pub col: usize,
}

/// A position: a block and a character offset inside it (`ch` counts characters, not bytes).
/// `cell` addresses one cell of a table block; `None` means the paragraph itself. A picture
/// block has the positions 0 (before) and 1 (after).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Cursor {
    pub block: usize,
    pub cell: Option<Cell>,
    pub ch: usize,
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

    pub(crate) fn of(e: &std::io::Error) -> DocError {
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

/// A word processor document with its file, caret, selection and undo history.
pub struct Doc {
    pub document: Document,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub cursor: Cursor,
    pub anchor: Option<Cursor>,
    /// Zoom factor [`ZOOM_MIN`] ..= [`ZOOM_MAX`].
    pub zoom: f32,
    /// A page (A4/Letter with margins) or a flowing phone view.
    pub page_view: bool,
    /// What opening found that the model cannot keep (see `ultar_office::docx::lossy_features`).
    pub lossy: Vec<&'static str>,
    /// The look the next typed text gets (Ctrl+B at an empty selection); cleared by movement.
    pub(crate) look: Option<Run>,
    pub(crate) hist: History,
    /// Bumped by every change of the document (the layout caches follow it).
    pub(crate) rev: u64,
}

impl Default for Doc {
    fn default() -> Doc {
        Doc::new()
    }
}

impl Doc {
    /// An empty document with one empty paragraph.
    pub fn new() -> Doc {
        Doc::from_document(Document {
            blocks: vec![Block::Para(Para::default())],
            ..Document::default()
        })
    }

    /// A document around a model (clean, caret at the start).
    pub fn from_document(document: Document) -> Doc {
        let mut d = Doc {
            document,
            path: None,
            dirty: false,
            cursor: Cursor::default(),
            anchor: None,
            zoom: 1.0,
            page_view: true,
            lossy: Vec::new(),
            look: None,
            hist: History::default(),
            rev: 0,
        };
        d.clamp();
        d
    }

    /// Opens a `.docx`, or a plain-text/Markdown file (`.odt` and the rest are refused:
    /// "not supported").
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
        let mut lossy = Vec::new();
        let document = match ext.as_str() {
            "docx" | "docm" | "dotx" => {
                lossy = ultar_office::docx::lossy_features(&bytes);
                ultar_office::docx::read(&bytes).map_err(|_| DocError::Unsupported)?
            }
            "md" | "markdown" => Document::from_markdown(&ultar_office::code::decode(&bytes).0),
            "txt" | "text" => Document::from_text(&ultar_office::code::decode(&bytes).0),
            _ => return Err(DocError::Unsupported),
        };
        let mut d = Doc::from_document(document);
        d.path = Some(path.to_path_buf());
        d.lossy = lossy;
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

    /// `true` when the file can be saved where it is (a `.docx`); everything else needs
    /// "save as".
    pub fn saves_in_place(&self) -> bool {
        self.path.as_deref().is_some_and(is_docx)
    }

    /// Saves to its file. A document opened with parts the model cannot keep first gets its
    /// original copied to `<name>.original.docx` (once), so nothing is lost for good.
    pub fn save(&mut self) -> Result<(), DocError> {
        let path = self
            .path
            .clone()
            .filter(|p| is_docx(p))
            .ok_or(DocError::BadName)?;
        if !self.lossy.is_empty() {
            keep_original(&path)?;
        }
        write_doc(&path, &self.document)?;
        self.hist.end(&self.document);
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
        self.lossy.clear();
        self.hist.end(&self.document);
        self.dirty = false;
        Ok(())
    }

    /// The `.docx` bytes of the document (autosave copy, export).
    pub fn docx_bytes(&self) -> Result<Vec<u8>, DocError> {
        ultar_office::docx::write(&self.document).map_err(|_| DocError::Io)
    }

    // ---- items and positions ---------------------------------------------

    /// The text of the item the position is in (a picture has none).
    pub fn item_text(&self, block: usize, cell: Option<Cell>) -> Option<String> {
        match (self.document.blocks.get(block)?, cell) {
            (Block::Para(p), None) => Some(p.text()),
            (Block::Table(t), Some(c)) => t.get(c.row).and_then(|r| r.get(c.col)).cloned(),
            _ => None,
        }
    }

    /// Number of characters of an item (a picture counts as one).
    pub fn item_len(&self, block: usize, cell: Option<Cell>) -> Option<usize> {
        match (self.document.blocks.get(block)?, cell) {
            (Block::Para(p), None) => Some(p.char_len()),
            (Block::Table(t), Some(c)) => t
                .get(c.row)
                .and_then(|r| r.get(c.col))
                .map(|s| s.chars().count()),
            (Block::Picture(_), None) => Some(1),
            _ => None,
        }
    }

    /// The item after `(block, cell)` in reading order (table cells row by row).
    pub fn item_after(&self, block: usize, cell: Option<Cell>) -> Option<(usize, Option<Cell>)> {
        if let (Some(Block::Table(t)), Some(c)) = (self.document.blocks.get(block), cell) {
            let row_len = t.get(c.row).map_or(0, Vec::len);
            if c.col + 1 < row_len {
                return Some((
                    block,
                    Some(Cell {
                        row: c.row,
                        col: c.col + 1,
                    }),
                ));
            }
            if c.row + 1 < t.len() {
                return Some((
                    block,
                    Some(Cell {
                        row: c.row + 1,
                        col: 0,
                    }),
                ));
            }
        }
        self.first_item(block + 1)
    }

    /// The item before `(block, cell)` in reading order.
    pub fn item_before(&self, block: usize, cell: Option<Cell>) -> Option<(usize, Option<Cell>)> {
        if let (Some(Block::Table(t)), Some(c)) = (self.document.blocks.get(block), cell) {
            if c.col > 0 {
                return Some((
                    block,
                    Some(Cell {
                        row: c.row,
                        col: c.col - 1,
                    }),
                ));
            }
            if c.row > 0 {
                let col = t.get(c.row - 1).map_or(0, |r| r.len().saturating_sub(1));
                return Some((
                    block,
                    Some(Cell {
                        row: c.row - 1,
                        col,
                    }),
                ));
            }
        }
        self.last_item(block.checked_sub(1)?)
    }

    fn first_item(&self, block: usize) -> Option<(usize, Option<Cell>)> {
        match self.document.blocks.get(block)? {
            Block::Table(_) => Some((block, Some(Cell::default()))),
            _ => Some((block, None)),
        }
    }

    fn last_item(&self, block: usize) -> Option<(usize, Option<Cell>)> {
        match self.document.blocks.get(block)? {
            Block::Table(t) => {
                let row = t.len().saturating_sub(1);
                let col = t.get(row).map_or(0, |r| r.len().saturating_sub(1));
                Some((block, Some(Cell { row, col })))
            }
            _ => Some((block, None)),
        }
    }

    /// Clamps the caret (and ensures the document has at least one block).
    pub fn clamp(&mut self) {
        if self.document.blocks.is_empty() {
            self.document.blocks.push(Block::Para(Para::default()));
        }
        self.cursor = self.clamped(self.cursor);
        self.anchor = self.anchor.map(|a| self.clamped(a));
    }

    fn clamped(&self, mut c: Cursor) -> Cursor {
        c.block = c.block.min(self.document.blocks.len() - 1);
        c.cell = match self.document.blocks.get(c.block) {
            Some(Block::Table(t)) => {
                let cell = c.cell.unwrap_or_default();
                let row = cell.row.min(t.len().saturating_sub(1));
                let col = cell
                    .col
                    .min(t.get(row).map_or(0, |r| r.len().saturating_sub(1)));
                Some(Cell { row, col })
            }
            _ => None,
        };
        c.ch = c.ch.min(self.item_len(c.block, c.cell).unwrap_or(0));
        c
    }

    // ---- selection --------------------------------------------------------

    fn order(&self, a: Cursor, b: Cursor) -> (Cursor, Cursor) {
        if (a.block, a.cell, a.ch) <= (b.block, b.cell, b.ch) {
            (a, b)
        } else {
            (b, a)
        }
    }

    /// The selection as (start, end) in document order; `None` without selection.
    pub fn selection(&self) -> Option<(Cursor, Cursor)> {
        let b = self.anchor.filter(|b| *b != self.cursor)?;
        Some(self.order(self.cursor, b))
    }

    /// The selected text (blocks joined by a line feed; table cells by a tab).
    pub fn selected_text(&self) -> String {
        let Some((a, b)) = self.selection() else {
            return String::new();
        };
        let part = |bl: usize, cell: Option<Cell>, from: usize, to: usize| -> String {
            self.item_text(bl, cell)
                .map(|t| t.chars().skip(from).take(to.saturating_sub(from)).collect())
                .unwrap_or_default()
        };
        if a.block == b.block && a.cell == b.cell {
            return part(a.block, a.cell, a.ch, b.ch);
        }
        let mut out: Vec<String> = Vec::new();
        for bi in a.block..=b.block {
            match self.document.blocks.get(bi) {
                Some(Block::Para(p)) => {
                    let n = p.char_len();
                    let from = if bi == a.block { a.ch } else { 0 };
                    let to = if bi == b.block { b.ch } else { n };
                    out.push(part(bi, None, from, to));
                }
                Some(Block::Table(t)) => {
                    for row in t {
                        out.push(row.join("\t"));
                    }
                }
                _ => {}
            }
        }
        out.join("\n")
    }

    /// Moves the caret; with `extend` the selection grows from where it was.
    pub fn set_pos(&mut self, c: Cursor, extend: bool) {
        if !extend {
            self.anchor = None;
        } else if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
        self.cursor = c;
        self.look = None;
        self.hist.end(&self.document);
        self.clamp();
    }

    /// Selects everything.
    pub fn select_all(&mut self) {
        let last = self.document.blocks.len().saturating_sub(1);
        let item = self.last_item(last).unwrap_or((last, None));
        let len = self.item_len(item.0, item.1).unwrap_or(0);
        self.cursor = Cursor {
            block: item.0,
            cell: item.1,
            ch: len,
        };
        self.anchor = self
            .first_item(0)
            .map(|(block, cell)| Cursor { block, cell, ch: 0 });
        self.hist.end(&self.document);
        self.look = None;
    }

    // ---- history ---------------------------------------------------------

    /// Opens an undo burst before the first change of it and marks the document changed.
    pub(crate) fn begin(&mut self) {
        self.hist.begin(&self.document, self.cursor, self.anchor);
        self.dirty = true;
        self.rev += 1;
    }

    /// Ends a typing burst (movement, formatting and other commands call this).
    pub fn burst_break(&mut self) {
        self.hist.end(&self.document);
    }

    pub fn can_undo(&self) -> bool {
        self.hist.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.hist.can_redo()
    }

    pub fn undo(&mut self) -> bool {
        let ok = self
            .hist
            .undo(&mut self.document, &mut self.cursor, &mut self.anchor);
        self.after_history(ok)
    }

    pub fn redo(&mut self) -> bool {
        let ok = self
            .hist
            .redo(&mut self.document, &mut self.cursor, &mut self.anchor);
        self.after_history(ok)
    }

    fn after_history(&mut self, ok: bool) -> bool {
        if ok {
            self.dirty = true;
            self.rev += 1;
            self.look = None;
            self.clamp();
        }
        ok
    }

    /// The counter of changes (a cache key for the layout).
    pub fn revision(&self) -> u64 {
        self.rev
    }
}

fn is_docx(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("docx"))
}

/// Copies `path` to `<stem>.original.docx` unless that exists.
fn keep_original(path: &Path) -> Result<(), DocError> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let copy = path.with_file_name(format!("{stem}.original.docx"));
    if copy.exists() || !path.exists() {
        return Ok(());
    }
    std::fs::copy(path, &copy)
        .map(|_| ())
        .map_err(|e| DocError::of(&e))
}

fn write_doc(path: &Path, d: &Document) -> Result<(), DocError> {
    if !is_docx(path) {
        return Err(DocError::Unsupported);
    }
    let bytes = ultar_office::docx::write(d).map_err(|_| DocError::Io)?;
    write_replacing(path, &bytes)
}

/// Writes `bytes` to a side file and renames it over `path`: a crash never leaves half a file.
pub(crate) fn write_replacing(path: &Path, bytes: &[u8]) -> Result<(), DocError> {
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
