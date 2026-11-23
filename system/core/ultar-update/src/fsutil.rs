//! Small durable-file helpers: atomic replacement, bounded reads, directory flush.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Path of the temporary file used while `path` is replaced.
fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

/// Flushes the directory entry changes of `dir` to the medium.
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

/// Flushes the directory that contains `path`.
pub fn sync_parent(path: &Path) -> io::Result<()> {
    match path.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(parent) => sync_dir(parent),
        None => sync_dir(Path::new(".")),
    }
}

/// Replaces `path` with `content` atomically: temporary file, flush, rename, directory flush.
/// After a power loss the file holds either the old or the new content, never a mixture.
pub fn write_atomic(path: &Path, content: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let tmp = temp_path(path);
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)?;
    file.write_all(content)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    sync_parent(path)
}

/// Reads a whole file of at most `max` bytes. `Ok(None)` when it does not exist; a larger file
/// is `InvalidData` (it is never read into memory).
pub fn read_bounded(path: &Path, max: u64) -> io::Result<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file too large"));
    }
    Ok(Some(bytes))
}

/// Removes a file; a file that is not there is not an error.
pub fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/value");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert!(!temp_path(&path).exists());
    }

    #[test]
    fn stale_temp_file_is_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("value");
        fs::write(temp_path(&path), b"half written garbage").unwrap();
        write_atomic(&path, b"good").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"good");
    }

    #[test]
    fn bounded_read_limits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        assert_eq!(read_bounded(&path, 4).unwrap(), None);
        fs::write(&path, b"1234").unwrap();
        assert_eq!(read_bounded(&path, 4).unwrap().unwrap(), b"1234");
        fs::write(&path, b"12345").unwrap();
        assert!(read_bounded(&path, 4).is_err());
    }

    #[test]
    fn remove_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        fs::write(&path, b"x").unwrap();
        remove_if_present(&path).unwrap();
        remove_if_present(&path).unwrap();
        assert!(!path.exists());
    }
}
