//! Files that survive power loss: written to a temporary name in the same
//! directory, synced, renamed over the old one, the directory synced.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Writes `bytes` to `path` atomically with permission bits `mode`. A
/// reader sees the old or the new content, never a mix.
pub fn write(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let temp = dir.join(format!(".{}.new", name.to_string_lossy()));
    // A leftover of an interrupted write is ours (the directory is the
    // launcher's own); never follow a link that could be in its place.
    match std::fs::symlink_metadata(&temp) {
        Ok(_) => std::fs::remove_file(&temp)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    // `create_new` (O_CREAT | O_EXCL) never follows a link at the name.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&temp)?;
    // The creation mask must not narrow (or widen) what the reader needs.
    std::fs::set_permissions(&temp, std::os::unix::fs::PermissionsExt::from_mode(mode))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temp, path)?;
    File::open(dir)?.sync_all()
}

/// Reads at most `max` bytes of the regular file `path` (not following a
/// link); a larger file is an error.
pub fn read(path: &Path, max: u64) -> std::io::Result<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.file_type().is_file() || meta.len() > max {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ulaunch-atomic-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_replace_and_keep_the_mode() {
        let dir = scratch("w");
        let file = dir.join("state");
        write(&file, b"one", 0o600).unwrap();
        write(&file, b"two", 0o644).unwrap();
        assert_eq!(read(&file, 16).unwrap(), b"two");
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
        // a planted link at the temporary name is removed, not followed
        let victim = dir.join("victim");
        std::fs::write(&victim, b"keep").unwrap();
        std::os::unix::fs::symlink(&victim, dir.join(".state.new")).unwrap();
        write(&file, b"three", 0o600).unwrap();
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
        assert_eq!(read(&file, 16).unwrap(), b"three");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_refuse_links_directories_and_large_files() {
        let dir = scratch("r");
        let file = dir.join("f");
        std::fs::write(&file, b"12345").unwrap();
        assert!(read(&file, 4).is_err());
        assert_eq!(read(&file, 5).unwrap(), b"12345");
        std::os::unix::fs::symlink(&file, dir.join("l")).unwrap();
        assert!(read(&dir.join("l"), 16).is_err());
        assert!(read(&dir, 16).is_err());
        assert!(write(Path::new("/"), b"x", 0o600).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
