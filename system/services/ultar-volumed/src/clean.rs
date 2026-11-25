//! The daily clean-up of the person's downloads folder (`storage.clean_downloads` and
//! `storage.clean_downloads_days`): regular files older than the given number of days,
//! owned by the person in front, are removed. Only that one folder – never recursive,
//! never through a symbolic link, never a file of someone else.

use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// Most entries examined in one run (a bound, like the update service's offline folder).
pub const MAX_ENTRIES: usize = 4096;

/// Seconds of one day.
const DAY: u64 = 86_400;

/// Removes the regular files in `dir` that are older than `days` days and belong to `uid`;
/// returns how many were removed. `now` is seconds since 1970. The folder itself must be a
/// real directory (a link planted there is refused); entries that are links, directories
/// or owned by someone else are left alone, and a file whose time cannot be read is kept.
pub fn clean_downloads(dir: &Path, uid: u32, days: u16, now: u64) -> usize {
    let Ok(meta) = std::fs::symlink_metadata(dir) else {
        return 0;
    };
    if !meta.is_dir() {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let cutoff = now.saturating_sub(u64::from(days) * DAY);
    let mut removed = 0;
    for entry in entries.take(MAX_ENTRIES).flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.uid() != uid {
            continue;
        }
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(now, |d| d.as_secs());
        if modified >= cutoff {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::time::Duration;

    const NOW: u64 = 1_700_000_000;

    /// A fresh folder below the system temp directory (never a real path).
    fn folder(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ultar-clean-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A regular file in `dir` whose modification time is `age` days before `NOW`.
    fn file(dir: &Path, name: &str, age_days: u64) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"data").unwrap();
        let when = UNIX_EPOCH + Duration::from_secs(NOW - age_days * DAY);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(when)
            .unwrap();
        path
    }

    fn me() -> u32 {
        rustix::process::getuid().as_raw()
    }

    #[test]
    fn only_old_regular_files_of_the_person_go() {
        let dir = folder("basic");
        let old = file(&dir, "old.bin", 40);
        let edge = file(&dir, "edge.bin", 30); // exactly 30 days: not older, kept
        let fresh = file(&dir, "fresh.bin", 2);
        let gone = clean_downloads(&dir, me(), 30, NOW);
        assert_eq!(gone, 1);
        assert!(!old.exists() && edge.exists() && fresh.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn links_directories_and_other_owners_are_left_alone() {
        let dir = folder("kinds");
        let target = file(&dir, "target.bin", 40);
        let link = dir.join("link.bin");
        symlink(&target, &link).unwrap();
        let sub = dir.join("sub");
        std::fs::create_dir(&sub).unwrap();
        let inside = file(&sub, "inside.bin", 40);
        // A file of someone else (only root may hand one over).
        let other = file(&dir, "other.bin", 40);
        let root = rustix::process::geteuid().as_raw() == 0;
        if root {
            std::os::unix::fs::chown(&other, Some(me() + 1), None).unwrap();
        }
        let gone = clean_downloads(&dir, me(), 30, NOW);
        // The old target goes; the link, the folder, its content and (as root) the other
        // owner's file stay. Without root the other file is ours and goes too.
        assert_eq!(gone, if root { 1 } else { 2 });
        // The link itself stays (its target went, so `exists` would follow it into nothing).
        assert!(std::fs::symlink_metadata(&link).is_ok() && sub.exists() && inside.exists());
        assert_eq!(other.exists(), root);
        assert!(!target.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_that_is_a_link_is_refused() {
        let dir = folder("linkroot");
        let real = dir.join("real");
        std::fs::create_dir(&real).unwrap();
        let old = file(&real, "old.bin", 40);
        let link = dir.join("downloads");
        symlink(&real, &link).unwrap();
        assert_eq!(clean_downloads(&link, me(), 30, NOW), 0);
        assert!(old.exists(), "never through the link");
        // A missing folder is quiet too.
        assert_eq!(clean_downloads(&dir.join("absent"), me(), 30, NOW), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_future_time_is_kept_and_the_days_setting_decides() {
        let dir = folder("time");
        let path = dir.join("future.bin");
        std::fs::write(&path, b"x").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(NOW + 10 * DAY))
            .unwrap();
        assert_eq!(clean_downloads(&dir, me(), 1, NOW), 0);
        assert!(path.exists());
        // With a larger window the same file is still not older than the cutoff.
        let old = file(&dir, "old.bin", 40);
        assert_eq!(clean_downloads(&dir, me(), 365, NOW), 0);
        assert!(old.exists());
        assert_eq!(clean_downloads(&dir, me(), 30, NOW), 1);
        assert!(!old.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
