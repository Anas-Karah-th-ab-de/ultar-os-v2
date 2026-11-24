//! The user's file areas (`[permissions] files` of the manifest): which ones a
//! cell gets and where they live on the host.
//!
//! An area of a **system app** (profile `app.system`, signed with a system
//! key) is bound at `/areas/<name>` (`ULTAR_AREAS=/areas`), writable but never
//! executable when the manifest says `:rw`, read-only otherwise; the app gets
//! the user's files group (`areas_group`) as its only supplementary group, the
//! area directories are `0770` launcher:`areas_group`. Store apps get no area
//! yet (they need the user's choice per area – open point, ultar-launcher.md).
//! `app:data` is the app's own `/data`; `removable` comes from the volume
//! service; any other name is unknown and left out (default deny).

use std::path::{Path, PathBuf};

/// The areas the launcher knows (names below `/areas`).
pub const KNOWN: [&str; 8] = [
    "home",
    "documents",
    "downloads",
    "media-pictures",
    "media-music",
    "media-videos",
    "desktop",
    "trash",
];

/// The known areas a manifest declares: `(name, writable)`, each once.
pub fn declared(files: &[String]) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    for entry in files {
        let (base, rw) = match entry.rsplit_once(':') {
            Some((base, "rw")) => (base, true),
            Some((base, "ro")) => (base, false),
            _ => (entry.as_str(), false),
        };
        let name = base.replace(':', "-");
        if !KNOWN.contains(&name.as_str()) {
            continue;
        }
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 |= rw,
            None => out.push((name, rw)),
        }
    }
    out
}

/// Host directory of area `name` below `root`.
pub fn host(root: &Path, name: &str) -> PathBuf {
    root.join(name)
}

/// Makes the area directories (`0770`, group `group`) below `root`; an area
/// that cannot be made is left out (the app shows it as missing).
pub fn ensure(root: &Path, group: u32) {
    use std::os::unix::fs::PermissionsExt;
    let make = |dir: &Path| {
        if std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
            return; // never follow a link planted there
        }
        let _ = std::fs::create_dir_all(dir);
        let _ = std::os::unix::fs::chown(dir, None, Some(group));
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o770));
    };
    make(root);
    for name in KNOWN {
        make(&host(root, name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_areas_with_their_mode() {
        let files: Vec<String> = [
            "home:rw",
            "documents",
            "media:pictures:rw",
            "media:music:ro",
            "removable:rw",
            "app:data:rw",
            "../etc:rw",
            "documents:rw",
            "secret",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            declared(&files),
            vec![
                ("home".to_owned(), true),
                ("documents".to_owned(), true),
                ("media-pictures".to_owned(), true),
                ("media-music".to_owned(), false),
            ]
        );
        assert!(declared(&[]).is_empty());
    }
}
