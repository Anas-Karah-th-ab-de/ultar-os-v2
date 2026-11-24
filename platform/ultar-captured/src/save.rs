//! Saving into the person's `Pictures` folder without following links.

use rustix::fs::OFlags;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Most files tried for a free name.
const MAX_TRIES: u32 = 10_000;

/// Why the picture was not saved.
#[derive(Debug)]
pub enum SaveError {
    /// The home or `Pictures` is missing, a link, or not owned by the person.
    Unsafe,
    /// Writing failed.
    Io(std::io::Error),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsafe => f.write_str("folder not usable"),
            Self::Io(e) => write!(f, "save: {e}"),
        }
    }
}

impl std::error::Error for SaveError {}

fn real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir())
}

/// Saves `bytes` as `Screenshot-<n>.png` in `home/Pictures` (created 0700 if missing),
/// owned by `uid` (`None`: leave the owner, tests and the person's own runs).
pub fn save(home: &Path, uid: Option<u32>, bytes: &[u8]) -> Result<PathBuf, SaveError> {
    if !real_dir(home) {
        return Err(SaveError::Unsafe);
    }
    let pictures = home.join("Pictures");
    if std::fs::symlink_metadata(&pictures).is_err() {
        std::fs::create_dir(&pictures).map_err(SaveError::Io)?;
        std::fs::set_permissions(&pictures, std::fs::Permissions::from_mode(0o700))
            .map_err(SaveError::Io)?;
        if let Some(uid) = uid {
            std::os::unix::fs::chown(&pictures, Some(uid), Some(uid)).map_err(SaveError::Io)?;
        }
    }
    if !real_dir(&pictures) {
        return Err(SaveError::Unsafe);
    }
    for n in 1..=MAX_TRIES {
        let path = pictures.join(format!("Screenshot-{n}.png"));
        let opened = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(OFlags::NOFOLLOW.bits() as i32)
            .open(&path);
        let mut file = match opened {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(SaveError::Io(e)),
        };
        let written = file.write_all(bytes).and_then(|()| file.sync_all());
        let owned = match uid {
            Some(uid) => std::os::unix::fs::chown(&path, Some(uid), Some(uid)),
            None => Ok(()),
        };
        if let Err(e) = written.and(owned) {
            let _ = std::fs::remove_file(&path);
            return Err(SaveError::Io(e));
        }
        return Ok(path);
    }
    Err(SaveError::Io(std::io::Error::from(
        std::io::ErrorKind::AlreadyExists,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_next_to_each_other_and_never_through_links() {
        let root = std::env::temp_dir().join(format!("captured-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("amal");
        std::fs::create_dir_all(&home).unwrap();
        let a = save(&home, None, b"one").unwrap();
        let b = save(&home, None, b"two").unwrap();
        assert!(
            a.ends_with("Pictures/Screenshot-1.png") && b.ends_with("Pictures/Screenshot-2.png")
        );
        assert_eq!(
            std::fs::metadata(&a).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(std::fs::read(&b).unwrap(), b"two");
        // A link in place of the next name: not followed, the name after it is used.
        std::os::unix::fs::symlink("/etc/passwd", home.join("Pictures/Screenshot-3.png")).unwrap();
        let c = save(&home, None, b"three").unwrap();
        assert!(c.ends_with("Screenshot-4.png"));
        // Pictures as a link, a missing home: refused.
        let other = root.join("badr");
        std::fs::create_dir_all(&other).unwrap();
        std::os::unix::fs::symlink("/tmp", other.join("Pictures")).unwrap();
        assert!(matches!(save(&other, None, b"x"), Err(SaveError::Unsafe)));
        assert!(matches!(
            save(&root.join("ghost"), None, b"x"),
            Err(SaveError::Unsafe)
        ));
        assert!(!SaveError::Unsafe.to_string().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
