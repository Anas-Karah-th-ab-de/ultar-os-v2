//! Files on their way between two devices: read in pieces on the sender, written to a
//! part file on the receiver and only given its name once size and SHA-256 match.

use crate::wire::{FileMeta, CHUNK};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Largest file the mesh carries (4 GiB).
pub const MAX_FILE: u64 = 4 * 1024 * 1024 * 1024;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A file name as it may be created in the inbox: the last path component only, no
/// control characters or separators, not hidden, at most 120 characters, never empty.
pub fn safe_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = last
        .chars()
        .filter(|c| !c.is_control() && *c != ':')
        .take(120)
        .collect();
    let clean = clean.trim().trim_start_matches('.').to_owned();
    if clean.is_empty() {
        "file".to_owned()
    } else {
        clean
    }
}

/// Size, name and hash of the file at `path`.
pub fn describe(path: &Path) -> std::io::Result<FileMeta> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    if !file.metadata()?.is_file() || size > MAX_FILE {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    Ok(FileMeta {
        name: safe_name(name),
        size,
        sha256: hex(&hash.finalize()),
    })
}

/// Number of pieces a file of `size` bytes has (an empty file has one empty piece).
pub fn pieces(size: u64) -> u32 {
    u32::try_from(size.div_ceil(CHUNK as u64).max(1)).unwrap_or(u32::MAX)
}

/// A file being sent.
#[derive(Debug)]
pub struct Outgoing {
    file: File,
    /// What was announced.
    pub meta: FileMeta,
}

impl Outgoing {
    /// Opens `path` for sending.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let meta = describe(path)?;
        Ok(Self {
            file: File::open(path)?,
            meta,
        })
    }

    /// Piece `index`; `None` past the end.
    pub fn piece(&mut self, index: u32) -> std::io::Result<Option<Vec<u8>>> {
        if index >= pieces(self.meta.size) {
            return Ok(None);
        }
        self.file
            .seek(SeekFrom::Start(u64::from(index) * CHUNK as u64))?;
        let mut buffer = vec![0u8; CHUNK];
        let mut filled = 0;
        while filled < CHUNK {
            let n = self.file.read(&mut buffer[filled..])?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        buffer.truncate(filled);
        Ok(Some(buffer))
    }
}

/// What a written piece led to.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// More pieces follow.
    More,
    /// The file is complete and verified at this path.
    Done(PathBuf),
}

/// Why receiving failed (the part file is removed).
#[derive(Debug, PartialEq, Eq)]
pub enum TransferError {
    /// A piece out of order, too much data, or a wrong hash.
    Corrupt,
    /// The inbox could not be written.
    Storage,
}

/// A file being received.
#[derive(Debug)]
pub struct Incoming {
    part: PathBuf,
    target: PathBuf,
    file: Option<File>,
    hash: Sha256,
    received: u64,
    next: u32,
    meta: FileMeta,
}

fn free_name(inbox: &Path, name: &str) -> PathBuf {
    let direct = inbox.join(name);
    if !direct.exists() {
        return direct;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    (2..10_000)
        .map(|n| inbox.join(format!("{stem} ({n}){extension}")))
        .find(|p| !p.exists())
        .unwrap_or(direct)
}

impl Incoming {
    /// Prepares to receive `meta` into `inbox` (created when missing).
    pub fn create(inbox: &Path, meta: &FileMeta) -> Result<Self, TransferError> {
        if meta.size > MAX_FILE {
            return Err(TransferError::Corrupt);
        }
        std::fs::create_dir_all(inbox).map_err(|_| TransferError::Storage)?;
        let target = free_name(inbox, &safe_name(&meta.name));
        let mut part = target.clone().into_os_string();
        part.push(".part");
        let part = PathBuf::from(part);
        let file = File::create(&part).map_err(|_| TransferError::Storage)?;
        Ok(Self {
            part,
            target,
            file: Some(file),
            hash: Sha256::new(),
            received: 0,
            next: 0,
            meta: meta.clone(),
        })
    }

    fn fail(&mut self, error: TransferError) -> TransferError {
        self.file = None;
        let _ = std::fs::remove_file(&self.part);
        error
    }

    /// Writes piece `index`.
    pub fn write(&mut self, index: u32, bytes: &[u8]) -> Result<Step, TransferError> {
        let fits = self.received + bytes.len() as u64 <= self.meta.size && bytes.len() <= CHUNK;
        if index != self.next || !fits {
            return Err(self.fail(TransferError::Corrupt));
        }
        let Some(file) = self.file.as_mut() else {
            return Err(TransferError::Storage);
        };
        if file.write_all(bytes).is_err() {
            return Err(self.fail(TransferError::Storage));
        }
        self.hash.update(bytes);
        self.received += bytes.len() as u64;
        self.next += 1;
        if self.received < self.meta.size {
            return Ok(Step::More);
        }
        if hex(&self.hash.clone().finalize()) != self.meta.sha256 {
            return Err(self.fail(TransferError::Corrupt));
        }
        self.file = None;
        if std::fs::rename(&self.part, &self.target).is_err() {
            return Err(self.fail(TransferError::Storage));
        }
        Ok(Step::Done(self.target.clone()))
    }

    /// Gives up (the part file is removed).
    pub fn abandon(&mut self) {
        let _ = self.fail(TransferError::Storage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mesh-tr-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn carry(source: &Path, inbox: &Path) -> Result<PathBuf, TransferError> {
        let mut out = Outgoing::open(source).unwrap();
        let mut inc = Incoming::create(inbox, &out.meta)?;
        let mut index = 0;
        loop {
            let piece = out.piece(index).unwrap().unwrap();
            match inc.write(index, &piece)? {
                Step::More => index += 1,
                Step::Done(path) => return Ok(path),
            }
        }
    }

    #[test]
    fn files_of_every_size_arrive_unchanged() {
        let d = dir("sizes");
        for size in [0usize, 1, CHUNK - 1, CHUNK, CHUNK + 1, 3 * CHUNK + 17] {
            let source = d.join(format!("in-{size}.bin"));
            let data: Vec<u8> = (0..size).map(|i| (i * 31 % 251) as u8).collect();
            std::fs::write(&source, &data).unwrap();
            let arrived = carry(&source, &d.join("inbox")).unwrap();
            assert_eq!(std::fs::read(arrived).unwrap(), data, "size {size}");
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_changed_piece_is_refused_and_leaves_nothing_behind() {
        let d = dir("bad");
        let source = d.join("a.txt");
        std::fs::write(&source, b"hello mesh").unwrap();
        let out = Outgoing::open(&source).unwrap();
        let inbox = d.join("inbox");
        let mut inc = Incoming::create(&inbox, &out.meta).unwrap();
        assert_eq!(inc.write(0, b"hello mesX"), Err(TransferError::Corrupt));
        assert_eq!(std::fs::read_dir(&inbox).unwrap().count(), 0);
        let mut inc = Incoming::create(&inbox, &out.meta).unwrap();
        assert_eq!(inc.write(1, b"hello mesh"), Err(TransferError::Corrupt));
        let mut inc = Incoming::create(&inbox, &out.meta).unwrap();
        assert_eq!(
            inc.write(0, b"hello mesh, and more"),
            Err(TransferError::Corrupt)
        );
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn names_cannot_leave_the_inbox_and_do_not_overwrite() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("..\\..\\boot.ini"), "boot.ini");
        assert_eq!(safe_name(".bashrc"), "bashrc");
        assert_eq!(safe_name("/"), "file");
        let d = dir("names");
        let source = d.join("note.txt");
        std::fs::write(&source, b"one").unwrap();
        let inbox = d.join("inbox");
        let first = carry(&source, &inbox).unwrap();
        let second = carry(&source, &inbox).unwrap();
        assert_eq!(first.file_name().unwrap(), "note.txt");
        assert_eq!(second.file_name().unwrap(), "note (2).txt");
        std::fs::remove_dir_all(d).unwrap();
    }
}
