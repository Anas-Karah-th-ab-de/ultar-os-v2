//! File operations of the store: atomic replace, bounded read, quarantine.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Largest settings file that is read (anything bigger is not ours).
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Copies of damaged files kept per file name.
pub const QUARANTINE_KEEP: usize = 5;
/// Directory below the settings directory that holds the copies.
pub const QUARANTINE_DIR: &str = "quarantine";
/// Suffix of files that are being written.
pub const TEMP_SUFFIX: &str = ".tmp";

/// What reading a settings file gave.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadOutcome {
    /// There is no such file.
    Missing,
    /// The file's text.
    Text(String),
    /// The file exists but is too large or not text.
    Damaged,
}

/// Replaces `dir/name` with `bytes` so that after a power loss the file
/// holds either the old or the new content, never a mix: the bytes go to a
/// temporary file that is synced, renamed over the target, and the
/// directory entry is synced as well.
pub fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let temp = dir.join(format!("{name}{TEMP_SUFFIX}"));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, dir.join(name))?;
        File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Reads `dir/name` as text, refusing oversized files.
pub fn read_file(dir: &Path, name: &str) -> io::Result<ReadOutcome> {
    let file = match File::open(dir.join(name)) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(ReadOutcome::Missing),
        Err(err) => return Err(err),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Ok(ReadOutcome::Damaged);
    }
    Ok(String::from_utf8(bytes).map_or(ReadOutcome::Damaged, ReadOutcome::Text))
}

fn quarantine_path(dir: &Path, name: &str, index: usize) -> PathBuf {
    dir.join(QUARANTINE_DIR).join(format!("{name}.{index}"))
}

/// Copies `dir/name` to `quarantine/<name>.0`, moving older copies up and
/// dropping the oldest beyond [`QUARANTINE_KEEP`]. Returns the copy's path
/// relative to `dir`.
pub fn quarantine(dir: &Path, name: &str) -> io::Result<String> {
    fs::create_dir_all(dir.join(QUARANTINE_DIR))?;
    let oldest = quarantine_path(dir, name, QUARANTINE_KEEP - 1);
    match fs::remove_file(&oldest) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(err),
        _ => {}
    }
    for index in (0..QUARANTINE_KEEP - 1).rev() {
        let from = quarantine_path(dir, name, index);
        match fs::rename(&from, quarantine_path(dir, name, index + 1)) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(err),
            _ => {}
        }
    }
    let target = quarantine_path(dir, name, 0);
    fs::copy(dir.join(name), &target)?;
    File::open(&target)?.sync_all()?;
    File::open(dir.join(QUARANTINE_DIR))?.sync_all()?;
    Ok(format!("{QUARANTINE_DIR}/{name}.0"))
}

/// Removes temporary files a crash left behind. Returns how many.
pub fn remove_stale_temp(dir: &Path) -> io::Result<usize> {
    let mut removed = 0;
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let is_temp = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(TEMP_SUFFIX));
        if is_temp && path.is_file() {
            fs::remove_file(&path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp_file() {
        let dir = TestDir::new("disk-write");
        write_atomic(dir.path(), "a.toml", b"one").unwrap();
        write_atomic(dir.path(), "a.toml", b"two").unwrap();
        assert_eq!(
            read_file(dir.path(), "a.toml").unwrap(),
            ReadOutcome::Text("two".into())
        );
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["a.toml"]);
    }

    #[test]
    fn atomic_write_reports_failure_and_cleans_up() {
        let dir = TestDir::new("disk-fail");
        // The target is a non-empty directory: the rename must fail.
        fs::create_dir_all(dir.path().join("a.toml").join("inner")).unwrap();
        assert!(write_atomic(dir.path(), "a.toml", b"x").is_err());
        assert!(!dir.path().join("a.toml.tmp").exists());
        assert!(write_atomic(&dir.path().join("missing"), "a.toml", b"x").is_err());
    }

    #[test]
    fn reading_missing_binary_and_oversized_files() {
        let dir = TestDir::new("disk-read");
        assert_eq!(read_file(dir.path(), "none").unwrap(), ReadOutcome::Missing);
        fs::write(dir.path().join("bin"), [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(read_file(dir.path(), "bin").unwrap(), ReadOutcome::Damaged);
        let big = File::create(dir.path().join("big")).unwrap();
        big.set_len(MAX_FILE_BYTES + 1).unwrap();
        assert_eq!(read_file(dir.path(), "big").unwrap(), ReadOutcome::Damaged);
        fs::create_dir(dir.path().join("dir")).unwrap();
        assert!(read_file(dir.path(), "dir").is_err());
    }

    #[test]
    fn quarantine_keeps_a_bounded_history() {
        let dir = TestDir::new("disk-quarantine");
        for round in 0..QUARANTINE_KEEP + 3 {
            fs::write(dir.path().join("s.toml"), format!("bad {round}")).unwrap();
            assert_eq!(
                quarantine(dir.path(), "s.toml").unwrap(),
                "quarantine/s.toml.0"
            );
        }
        let copies = fs::read_dir(dir.path().join(QUARANTINE_DIR))
            .unwrap()
            .count();
        assert_eq!(copies, QUARANTINE_KEEP);
        let newest = fs::read_to_string(quarantine_path(dir.path(), "s.toml", 0)).unwrap();
        assert_eq!(newest, format!("bad {}", QUARANTINE_KEEP + 2));
        let oldest =
            fs::read_to_string(quarantine_path(dir.path(), "s.toml", QUARANTINE_KEEP - 1)).unwrap();
        assert_eq!(oldest, "bad 3");
        // The original stays where it was: a copy, not a move.
        assert!(dir.path().join("s.toml").exists());
        assert!(quarantine(dir.path(), "absent").is_err());
    }

    #[test]
    fn stale_temp_files_are_removed() {
        let dir = TestDir::new("disk-temp");
        fs::write(dir.path().join("a.toml.tmp"), "x").unwrap();
        fs::write(dir.path().join("b.toml"), "x").unwrap();
        fs::create_dir(dir.path().join("c.tmp")).unwrap();
        assert_eq!(remove_stale_temp(dir.path()).unwrap(), 1);
        assert!(dir.path().join("b.toml").exists());
        assert!(dir.path().join("c.tmp").exists());
        assert!(remove_stale_temp(&dir.path().join("missing")).is_err());
    }
}
