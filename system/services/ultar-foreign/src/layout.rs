//! Where the service keeps everything (one root, owned by the service, `0755`):
//!
//! | Path | What | Mode |
//! |---|---|---|
//! | `<root>/linux` | the Linux area (component „Linux programs“) | written only in the installation cell |
//! | `<root>/inbox` | a copy of the file being installed or inspected | `0700`, the service |
//! | `<root>/icons` | the apps' symbols (`<id>.png`/`.svg`), for the surfaces | `0755`/`0644` |
//! | `<root>/setup` | each Windows program's own installer (`<id>/<file>`), read-only in its cell | `0755`/`0644` |
//! | `<root>/state.toml` | the apps from other systems and what was granted | `0600`, the service |

use std::path::{Path, PathBuf};

/// The service's places.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The root.
    pub root: PathBuf,
}

impl Layout {
    /// The places below `root`.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
        }
    }

    /// The Linux area.
    pub fn runtime(&self) -> PathBuf {
        self.root.join("linux")
    }

    /// Copies of files being worked on.
    pub fn inbox(&self) -> PathBuf {
        self.root.join("inbox")
    }

    /// The Windows programs' own installers (`setup/<app id>/`), bound read-only into
    /// each one's cell by the launcher (`[paths] foreign_setup`).
    pub fn setup(&self) -> PathBuf {
        self.root.join("setup")
    }

    /// The apps' symbols.
    pub fn icons(&self) -> PathBuf {
        self.root.join("icons")
    }

    /// The state file.
    pub fn state(&self) -> PathBuf {
        self.root.join("state.toml")
    }

    /// Whether the Linux area is installed: its root holds `usr` and the package database.
    pub fn runtime_installed(&self) -> bool {
        let r = self.runtime();
        r.join("usr/bin").is_dir() && r.join("var/lib/dpkg/status").is_file()
    }

    /// Whether the area has the component „Windows programs“ (Wine and the app wrapper).
    pub fn windows_installed(&self) -> bool {
        let r = self.runtime();
        self.runtime_installed()
            && r.join(crate::windows::WINE.trim_start_matches('/'))
                .is_file()
            && r.join(crate::windows::WRAPPER.trim_start_matches('/'))
                .is_file()
    }

    /// The version of the Linux area (`/etc/ultar-runtime`, written by its build).
    pub fn runtime_version(&self) -> String {
        std::fs::read_to_string(self.runtime().join("etc/ultar-runtime"))
            .ok()
            .and_then(|t| {
                t.lines()
                    .find_map(|l| l.strip_prefix("version=").map(str::to_owned))
            })
            .filter(|v| v.len() <= 64 && !v.chars().any(char::is_control))
            .unwrap_or_default()
    }

    /// Makes the service's own directories (`inbox` `0700`, `icons` `0755`).
    pub fn ensure(&self) -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        for (dir, mode) in [
            (self.inbox(), 0o700),
            (self.icons(), 0o755),
            (self.setup(), 0o755),
        ] {
            std::fs::create_dir_all(&dir)?;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode))?;
        }
        Ok(())
    }
}

/// Bytes below `dir` (files only, links not followed; at most `limit` entries counted).
pub fn size_of(dir: &Path, limit: usize) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![dir.to_owned()];
    let mut seen = 0usize;
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > limit {
                return total;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_and_an_absent_area() {
        let l = Layout::new(Path::new("/nonexistent/foreign"));
        assert_eq!(l.runtime(), Path::new("/nonexistent/foreign/linux"));
        assert!(!l.runtime_installed());
        assert_eq!(l.runtime_version(), "");
        assert_eq!(size_of(Path::new("/nonexistent"), 10), 0);
    }
}
