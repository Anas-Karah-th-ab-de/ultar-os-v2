//! The document: the office library's [`Buffer`] with an index of line starts (the view asks
//! for single lines), its file, and saving through a temporary file and a rename.

use std::io::Write;
use std::path::{Path, PathBuf};
use ultar_office::code::Buffer;
use ultar_textview::{Pos, Source};

/// Largest file opened (bigger files stay closed with a message).
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;
/// One step of indentation (Enter keeps the indentation of the line).
pub const INDENT: &str = "    ";

/// Why a file could not be opened or saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocError {
    /// The system refused (permission, read-only place).
    Denied,
    /// The file or its folder does not exist.
    Missing,
    /// A file of that name exists (save as).
    Exists,
    /// A name that cannot be used.
    BadName,
    /// Larger than [`MAX_BYTES`].
    TooBig,
    /// Any other input/output error.
    Io,
}

impl DocError {
    /// The message key of the reason.
    pub fn key(self) -> &'static str {
        match self {
            DocError::Denied => "editor.reason.denied",
            DocError::Missing => "editor.reason.missing",
            DocError::Exists => "editor.reason.exists",
            DocError::BadName => "editor.reason.name",
            DocError::TooBig | DocError::Io => "editor.reason.io",
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

/// An open document.
pub struct Doc {
    /// Text, caret, selection, undo.
    pub buffer: Buffer,
    /// The file; `None` for a new document not saved yet.
    pub path: Option<PathBuf>,
    starts: Vec<usize>,
}

impl Source for Doc {
    fn lines(&self) -> usize {
        self.starts.len()
    }
    fn line(&self, i: usize) -> &str {
        let Some(&start) = self.starts.get(i) else {
            return "";
        };
        let end = self
            .starts
            .get(i + 1)
            .map_or(self.buffer.text.len(), |next| next - 1);
        self.buffer.text.get(start..end).unwrap_or("")
    }
}

impl Doc {
    /// An empty, new document.
    pub fn new() -> Doc {
        Doc::from_buffer(Buffer::new(""), None)
    }

    fn from_buffer(buffer: Buffer, path: Option<PathBuf>) -> Doc {
        let mut d = Doc {
            buffer,
            path,
            starts: Vec::new(),
        };
        d.reindex();
        d
    }

    /// Opens `path` (its encoding and line ends are kept for saving).
    pub fn open(path: &Path) -> Result<Doc, DocError> {
        let size = std::fs::metadata(path).map_err(|e| DocError::of(&e))?.len();
        if size > MAX_BYTES {
            return Err(DocError::TooBig);
        }
        let bytes = std::fs::read(path).map_err(|e| DocError::of(&e))?;
        Ok(Doc::from_buffer(
            Buffer::from_bytes(&bytes),
            Some(path.to_path_buf()),
        ))
    }

    /// The name shown in the title (`None`: a new document).
    pub fn name(&self) -> Option<String> {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
    }

    /// `true` when there are unsaved changes.
    pub fn dirty(&self) -> bool {
        self.buffer.dirty
    }

    /// Rebuilds the line index (after every change of the text).
    pub fn reindex(&mut self) {
        self.starts.clear();
        self.starts.push(0);
        let text = &self.buffer.text;
        self.starts
            .extend(text.match_indices('\n').map(|(i, _)| i + 1));
    }

    /// The caret as line and byte in the line.
    pub fn caret(&self) -> Pos {
        self.pos_of(self.buffer.cur)
    }

    /// The selection as two places, if any.
    pub fn selection(&self) -> Option<(Pos, Pos)> {
        self.buffer
            .selection()
            .map(|(a, b)| (self.pos_of(a), self.pos_of(b)))
    }

    /// A byte offset in the text as line and byte in the line.
    pub fn pos_of(&self, at: usize) -> Pos {
        let line = self.starts.partition_point(|s| *s <= at).saturating_sub(1);
        Pos::new(line, at - self.starts[line.min(self.starts.len() - 1)])
    }

    /// A place as byte offset in the text (clamped to the line, on a character boundary).
    pub fn offset_of(&self, p: Pos) -> usize {
        let line = p.line.min(self.starts.len() - 1);
        let text = self.line(line);
        let mut byte = p.byte.min(text.len());
        while !text.is_char_boundary(byte) {
            byte -= 1;
        }
        self.starts[line] + byte
    }

    /// Line and column (in characters, from 1) of the caret, for the status line.
    pub fn line_col(&self) -> (usize, usize) {
        let p = self.caret();
        let col = self
            .line(p.line)
            .get(..p.byte)
            .map_or(0, |s| s.chars().count());
        (p.line + 1, col + 1)
    }

    /// Saves to its file: a temporary file next to it, synced, then renamed over it.
    pub fn save(&mut self) -> Result<(), DocError> {
        let path = self.path.clone().ok_or(DocError::BadName)?;
        write_replacing(&path, &self.buffer.to_bytes())?;
        self.buffer.dirty = false;
        Ok(())
    }

    /// Saves a new document as `path` (which must not exist yet).
    pub fn save_as(&mut self, path: &Path) -> Result<(), DocError> {
        if path.exists() {
            return Err(DocError::Exists);
        }
        write_replacing(path, &self.buffer.to_bytes())?;
        self.path = Some(path.to_path_buf());
        self.buffer.dirty = false;
        Ok(())
    }
}

impl Default for Doc {
    fn default() -> Doc {
        Doc::new()
    }
}

fn write_replacing(path: &Path, bytes: &[u8]) -> Result<(), DocError> {
    let name = path
        .file_name()
        .ok_or(DocError::BadName)?
        .to_string_lossy()
        .into_owned();
    let part = path.with_file_name(format!(".{name}.part"));
    let written = std::fs::File::create(&part).and_then(|mut f| {
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

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ultar-editor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap_or_default();
        d
    }

    #[test]
    fn lines_and_places() {
        let mut d = Doc::new();
        d.buffer.insert("سطر أول\nsecond\n");
        d.reindex();
        assert_eq!(d.lines(), 3);
        assert_eq!((d.line(0), d.line(1), d.line(2)), ("سطر أول", "second", ""));
        assert_eq!(d.caret(), Pos::new(2, 0));
        let off = d.offset_of(Pos::new(1, 3));
        assert_eq!(d.pos_of(off), Pos::new(1, 3));
        // inside a two-byte letter: moved back to its start
        assert_eq!(d.offset_of(Pos::new(0, 1)), 0);
        d.buffer.cur = d.offset_of(Pos::new(0, "سطر".len()));
        assert_eq!(d.line_col(), (1, 4));
    }

    #[test]
    fn open_edit_save_keeps_the_encoding() {
        let d0 = dir("save");
        let path = d0.join("a.txt");
        std::fs::write(&path, b"one\r\ntwo\r\n").unwrap_or_default();
        let mut d = Doc::open(&path).unwrap_or_default();
        assert_eq!(d.lines(), 3);
        d.buffer.cur = d.buffer.text.len();
        d.buffer.insert("ثلاثة");
        d.reindex();
        assert!(d.dirty());
        assert_eq!(d.save(), Ok(()));
        assert!(!d.dirty());
        let saved = std::fs::read(&path).unwrap_or_default();
        assert_eq!(saved, "one\r\ntwo\r\nثلاثة".as_bytes());
        assert!(!d0.join(".a.txt.part").exists());
        let mut n = Doc::new();
        assert_eq!(n.save_as(&path), Err(DocError::Exists));
        assert_eq!(n.save_as(&d0.join("b.txt")), Ok(()));
        assert_eq!(n.name().as_deref(), Some("b.txt"));
        let _ = std::fs::remove_dir_all(&d0);
    }

    #[test]
    fn missing_files_say_so() {
        assert_eq!(
            Doc::open(Path::new("/nonexistent/x.txt")).err(),
            Some(DocError::Missing)
        );
    }
}
