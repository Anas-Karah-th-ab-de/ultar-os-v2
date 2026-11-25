//! Directories the launcher owns: its state, the root of the apps' data,
//! the cells' mount points – and each app's own data directory (`0700`,
//! owned by the app's user).
//!
//! Every directory is checked, not trusted: a link, a non-directory, a
//! directory of someone else or one that others may write is refused.
//! The parents are the launcher's own `0700` directories, so nobody else
//! can plant a link between the check and the use.

use std::fmt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Why a directory is refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DirError {
    /// It is a link or not a directory.
    NotADirectory,
    /// Someone else owns it, or others may write to it.
    Untrusted,
    /// Creating, changing or removing failed.
    Io(std::io::ErrorKind),
}

impl fmt::Display for DirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotADirectory => f.write_str("launcher.dir.not_a_directory"),
            Self::Untrusted => f.write_str("launcher.dir.untrusted"),
            Self::Io(kind) => write!(f, "launcher.dir.io ({kind})"),
        }
    }
}

impl std::error::Error for DirError {}

impl From<std::io::Error> for DirError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.kind())
    }
}

fn create_dir(path: &Path, mode: u32) -> Result<(), DirError> {
    match std::fs::DirBuilder::new().mode(mode).create(path) {
        Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => Err(error.into()),
        _ => Ok(()),
    }
}

/// Makes sure `path` is a directory of this process's user that nobody
/// else may write (created `0700` if missing; group/other write bits of an
/// existing one are a refusal, not something to repair silently).
pub fn ensure_private_dir(path: &Path) -> Result<(), DirError> {
    create_dir(path, 0o700)?;
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.file_type().is_dir() {
        return Err(DirError::NotADirectory);
    }
    if meta.uid() != ultar_sys::euid() || meta.mode() & 0o022 != 0 {
        return Err(DirError::Untrusted);
    }
    Ok(())
}

/// Like [`ensure_private_dir`], but `0755`: others may pass through (the publish root).
pub fn ensure_public_dir(path: &Path) -> Result<(), DirError> {
    create_dir(path, 0o755)?;
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.file_type().is_dir() {
        return Err(DirError::NotADirectory);
    }
    if meta.uid() != ultar_sys::euid() || meta.mode() & 0o022 != 0 {
        return Err(DirError::Untrusted);
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// Who owns an app's data directory.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Owner {
    /// User id.
    pub uid: u32,
    /// Group id.
    pub gid: u32,
}

/// Creates (or checks) the data directory of an app below `root`:
/// `0700`, owned by `owner` (`None`: this process's user – only where the
/// platform cannot switch ids and the operator accepted that).
pub fn ensure_app_dir(root: &Path, name: &str, owner: Option<Owner>) -> Result<PathBuf, DirError> {
    app_dir(root, name, owner, 0o700)
}

/// Creates (or checks) the publish directory of an app below `root`: like its data
/// directory, but `0755` – the app writes, every other user may read.
pub fn ensure_publish_dir(
    root: &Path,
    name: &str,
    owner: Option<Owner>,
) -> Result<PathBuf, DirError> {
    app_dir(root, name, owner, 0o755)
}

fn app_dir(root: &Path, name: &str, owner: Option<Owner>, mode: u32) -> Result<PathBuf, DirError> {
    if name.is_empty() || name.contains('/') || name.starts_with('.') {
        return Err(DirError::NotADirectory);
    }
    let path = root.join(name);
    create_dir(&path, mode)?;
    let meta = std::fs::symlink_metadata(&path)?;
    if !meta.file_type().is_dir() {
        return Err(DirError::NotADirectory);
    }
    if let Some(owner) = owner {
        if meta.uid() != owner.uid || meta.gid() != owner.gid {
            std::os::unix::fs::lchown(&path, Some(owner.uid), Some(owner.gid))?;
        }
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))?;
    let meta = std::fs::symlink_metadata(&path)?;
    let wanted = owner.map_or(ultar_sys::euid(), |o| o.uid);
    if meta.uid() != wanted || meta.mode() & 0o7777 != mode {
        return Err(DirError::Untrusted);
    }
    Ok(path)
}

/// Removes the data directory `name` below `root` with everything in it
/// (links inside are removed, never followed). A missing one is fine.
pub fn remove_app_dir(root: &Path, name: &str) -> Result<(), DirError> {
    if name.is_empty() || name.contains('/') || name.starts_with('.') {
        return Err(DirError::NotADirectory);
    }
    let path = root.join(name);
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(meta) if !meta.file_type().is_dir() => Err(DirError::NotADirectory),
        Ok(_) => std::fs::remove_dir_all(&path).map_err(Into::into),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ulaunch-dir-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn private_dirs_are_created_and_checked() {
        let dir = scratch("p");
        ensure_private_dir(&dir).unwrap();
        assert_eq!(std::fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);
        ensure_private_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(ensure_private_dir(&dir), Err(DirError::Untrusted));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let link = scratch("p-link");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        assert_eq!(ensure_private_dir(&link), Err(DirError::NotADirectory));
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn app_dirs_are_0700_and_removed_without_following_links() {
        let root = scratch("a");
        ensure_private_dir(&root).unwrap();
        let path = ensure_app_dir(&root, "camera", None).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o7777, 0o700);
        // a widened mode is put back
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        ensure_app_dir(&root, "camera", None).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o7777, 0o700);
        let outside = scratch("a-outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep"), b"k").unwrap();
        std::os::unix::fs::symlink(&outside, path.join("escape")).unwrap();
        remove_app_dir(&root, "camera").unwrap();
        assert!(!path.exists());
        assert!(outside.join("keep").exists());
        remove_app_dir(&root, "camera").unwrap();
        // a link in place of the data directory is refused
        std::os::unix::fs::symlink(&outside, root.join("evil")).unwrap();
        assert_eq!(
            ensure_app_dir(&root, "evil", None),
            Err(DirError::NotADirectory)
        );
        assert_eq!(remove_app_dir(&root, "evil"), Err(DirError::NotADirectory));
        for bad in ["", "a/b", ".hidden", ".."] {
            assert!(ensure_app_dir(&root, bad, None).is_err());
            assert!(remove_app_dir(&root, bad).is_err());
        }
        let _ = std::fs::remove_dir_all(&outside);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn app_dirs_belong_to_the_app_when_ids_can_be_set() {
        if ultar_sys::euid() != 0 {
            return; // changing owners needs root (checked by the process tests as root)
        }
        let root = scratch("o");
        ensure_private_dir(&root).unwrap();
        let owner = Owner {
            uid: 20999,
            gid: 20999,
        };
        let path = ensure_app_dir(&root, "x", Some(owner)).unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        assert_eq!((meta.uid(), meta.gid()), (20999, 20999));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn publish_dirs_are_readable_by_others_and_their_root_is_checked() {
        let root = scratch("pub");
        ensure_public_dir(&root).unwrap();
        assert_eq!(std::fs::metadata(&root).unwrap().mode() & 0o777, 0o755);
        let dir = ensure_publish_dir(&root, "store", None).unwrap();
        assert_eq!(std::fs::metadata(&dir).unwrap().mode() & 0o777, 0o755);
        // the data directory of the same name rule stays private
        let data = ensure_app_dir(&root, "data", None).unwrap();
        assert_eq!(std::fs::metadata(&data).unwrap().mode() & 0o777, 0o700);
        assert!(ensure_publish_dir(&root, "../x", None).is_err());
        assert!(ensure_publish_dir(&root, ".hidden", None).is_err());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(ensure_public_dir(&root), Err(DirError::Untrusted));
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
    }
}
