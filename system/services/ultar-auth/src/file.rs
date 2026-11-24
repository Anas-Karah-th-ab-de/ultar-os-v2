//! Checked, atomically written text files: a first line `#<kind> crc32=<8 hex>` over the
//! rest; written to a temp file, synced, renamed over the old one, the directory synced.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Largest file read (an account file of 64 users is a few kilobytes).
pub const MAX_FILE: u64 = 256 * 1024;

/// Why a file could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    /// Reading or writing failed (kind of the I/O error).
    Io(std::io::ErrorKind),
    /// Too large, not UTF-8, header or checksum wrong: the file is damaged.
    Damaged,
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileError::Io(kind) => write!(f, "i/o error: {kind:?}"),
            FileError::Damaged => f.write_str("damaged file"),
        }
    }
}

impl std::error::Error for FileError {}

impl From<std::io::Error> for FileError {
    fn from(e: std::io::Error) -> FileError {
        FileError::Io(e.kind())
    }
}

/// CRC-32 (IEEE 802.3, reflected).
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Reads `path`; `Ok(None)` when it does not exist.
pub fn read(path: &Path, kind: &str) -> Result<Option<String>, FileError> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut raw = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut raw)?;
    if raw.len() as u64 > MAX_FILE {
        return Err(FileError::Damaged);
    }
    let text = String::from_utf8(raw).map_err(|_| FileError::Damaged)?;
    let (head, body) = text.split_once('\n').ok_or(FileError::Damaged)?;
    let expected = head
        .strip_prefix('#')
        .and_then(|h| h.strip_prefix(kind))
        .and_then(|h| h.strip_prefix(" crc32="))
        .filter(|h| h.len() == 8)
        .and_then(|h| u32::from_str_radix(h, 16).ok())
        .ok_or(FileError::Damaged)?;
    if crc32(body.as_bytes()) != expected {
        return Err(FileError::Damaged);
    }
    Ok(Some(body.to_owned()))
}

/// Writes `body` to `path` atomically with its header; the file is readable by its owner
/// only (`0600`).
pub fn write(path: &Path, kind: &str, body: &str) -> Result<(), FileError> {
    let dir = path
        .parent()
        .ok_or(FileError::Io(std::io::ErrorKind::NotFound))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(FileError::Io(std::io::ErrorKind::InvalidInput))?;
    let temp = dir.join(format!(".{name}.tmp"));
    let content = format!("#{kind} crc32={:08x}\n{body}", crc32(body.as_bytes()));
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&temp, path)?;
    File::open(dir)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ultar-auth-file-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn round_trip_and_damage_is_found() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let p = dir().join("f.txt");
        assert_eq!(read(&p, "k").unwrap(), None);
        write(&p, "k", "user a\n").unwrap();
        assert_eq!(read(&p, "k").unwrap().as_deref(), Some("user a\n"));
        assert_eq!(read(&p, "other"), Err(FileError::Damaged));
        let mut bytes = fs::read(&p).unwrap();
        let last = bytes.len() - 2;
        bytes[last] ^= 1;
        fs::write(&p, &bytes).unwrap();
        assert_eq!(read(&p, "k"), Err(FileError::Damaged));
        fs::write(&p, b"no header").unwrap();
        assert_eq!(read(&p, "k"), Err(FileError::Damaged));
        fs::write(&p, vec![b'x'; MAX_FILE as usize + 10]).unwrap();
        assert_eq!(read(&p, "k"), Err(FileError::Damaged));
        fs::write(&p, [0xff, b'\n']).unwrap();
        assert_eq!(read(&p, "k"), Err(FileError::Damaged));
        assert!(!FileError::Damaged.to_string().is_empty());
        let _ = fs::remove_dir_all(dir());
    }
}
