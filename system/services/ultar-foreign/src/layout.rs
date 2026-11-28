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
        area_ok(&self.runtime())
    }

    /// Whether the area has the component „Windows programs“: its build wrote `components=windows`
    /// (after checking Wine in the area), the app wrapper is there and `wine` is a file or a link
    /// (Ubuntu's is a link through `/etc/alternatives`, which only resolves inside the area).
    pub fn windows_installed(&self) -> bool {
        let r = self.runtime();
        let present = |p: &str| {
            std::fs::symlink_metadata(r.join(p.trim_start_matches('/')))
                .is_ok_and(|m| m.is_file() || m.file_type().is_symlink())
        };
        self.runtime_installed()
            && self
                .runtime_field("components")
                .split(',')
                .any(|c| c.trim() == "windows")
            && present(crate::windows::WINE)
            && r.join(crate::windows::WRAPPER.trim_start_matches('/'))
                .is_file()
    }

    /// Whether the component „Android apps“ is in the Linux area (its start program).
    pub fn android_installed(&self) -> bool {
        self.runtime()
            .join(crate::apk::RUN.trim_start_matches('/'))
            .is_file()
    }

    /// The version of the Linux area (`/etc/ultar-runtime`, written by its build).
    pub fn runtime_version(&self) -> String {
        self.runtime_field("version")
    }

    /// One `key=value` line of `/etc/ultar-runtime` (empty when missing, long or odd).
    fn runtime_field(&self, key: &str) -> String {
        field_in(&self.runtime(), key)
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

/// Whether `area` is a Linux area: its root holds `usr` and the package database.
pub fn area_ok(area: &Path) -> bool {
    area.join("usr/bin").is_dir() && area.join("var/lib/dpkg/status").is_file()
}

/// One `key=value` line of `<area>/etc/ultar-runtime` (empty when missing, long or odd).
pub fn field_in(area: &Path, key: &str) -> String {
    std::fs::read_to_string(area.join("etc/ultar-runtime"))
        .ok()
        .and_then(|t| {
            t.lines().find_map(|l| {
                l.strip_prefix(key)
                    .and_then(|v| v.strip_prefix('='))
                    .map(str::to_owned)
            })
        })
        .filter(|v| v.len() <= 64 && !v.chars().any(char::is_control))
        .unwrap_or_default()
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

    #[test]
    fn the_windows_component_needs_its_mark_wine_and_the_wrapper() {
        let dir = std::env::temp_dir().join(format!("ultar-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let l = Layout::new(&dir);
        let r = l.runtime();
        for d in ["usr/bin", "var/lib/dpkg", "etc", "usr/lib/ultar/wine"] {
            std::fs::create_dir_all(r.join(d)).unwrap();
        }
        std::fs::write(r.join("var/lib/dpkg/status"), "").unwrap();
        std::fs::write(
            r.join("etc/ultar-runtime"),
            "version=26.04.1\ncomponents=\n",
        )
        .unwrap();
        // Ubuntu's wine: a link through /etc/alternatives, dangling from outside the area.
        std::os::unix::fs::symlink("/etc/alternatives/ultar-test-wine", r.join("usr/bin/wine"))
            .unwrap();
        std::fs::write(r.join("usr/lib/ultar/wine/ultar-wine-app"), "#!/bin/sh\n").unwrap();
        assert!(l.runtime_installed());
        assert_eq!(l.runtime_version(), "26.04.1");
        assert!(!l.windows_installed(), "no component mark");
        std::fs::write(
            r.join("etc/ultar-runtime"),
            "version=26.04.1\ncomponents=windows\n",
        )
        .unwrap();
        assert!(l.windows_installed());
        std::fs::remove_file(r.join("usr/lib/ultar/wine/ultar-wine-app")).unwrap();
        assert!(!l.windows_installed(), "no wrapper");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
