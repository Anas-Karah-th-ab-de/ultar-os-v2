//! The compositor's rights for app user ids.
//!
//! The compositor tells clients apart by the kernel's peer credentials
//! (`SO_PEERCRED`) and looks up their rights by user id. Since every app
//! has its own id, its per-uid limits apply per app (closing compositor
//! findings N7/K9), and its `ui.*` permissions become lines of this file:
//!
//! ```text
//! 20003 = "ui.capture,ui.overlay"
//! ```
//!
//! The compositor reads the file (`--app-rights`) whenever it changed,
//! accepts only ids inside the app range (`--app-uids`) and only a file
//! owned by the launcher that nobody else may write; a bad file gives no
//! app any right (fail closed). Written atomically, mode `0644`.

use crate::perm::PermissionSet;
use std::path::Path;

/// First line of the file.
const HEADER: &str = "# Written by ultar-launcher: ui permissions of app user ids. Do not edit.\n";

/// The file's text for `(uid, granted)` pairs (only `ui.*` permissions;
/// ids without one are left out).
pub fn render(apps: &[(u32, PermissionSet)]) -> String {
    let mut lines: Vec<(u32, String)> = apps
        .iter()
        .filter_map(|(uid, granted)| {
            let names: Vec<&str> = granted
                .iter()
                .filter(|p| p.is_ui())
                .map(|p| p.name())
                .collect();
            (!names.is_empty()).then(|| (*uid, names.join(",")))
        })
        .collect();
    lines.sort_unstable();
    let mut text = HEADER.to_owned();
    for (uid, names) in lines {
        text.push_str(&format!("{uid} = \"{names}\"\n"));
    }
    text
}

/// Writes the file at `path`.
pub fn write(path: &Path, apps: &[(u32, PermissionSet)]) -> std::io::Result<()> {
    crate::atomic::write(path, render(apps).as_bytes(), 0o644)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perm::Permission;

    #[test]
    fn only_ui_permissions_of_app_ids_are_written() {
        let apps = [
            (
                20005,
                PermissionSet::of(&[
                    Permission::Network,
                    Permission::UiOverlay,
                    Permission::UiCapture,
                ]),
            ),
            (20001, PermissionSet::of(&[Permission::Background])),
            (20002, PermissionSet::of(&[Permission::UiShell])),
        ];
        let text = render(&apps);
        assert_eq!(
            text,
            format!("{HEADER}20002 = \"ui.shell\"\n20005 = \"ui.capture,ui.overlay\"\n")
        );
        assert_eq!(render(&[]), HEADER);
    }

    #[test]
    fn the_compositor_reads_what_is_written() {
        let apps = [(
            20005,
            PermissionSet::of(&[Permission::UiSystemDialog, Permission::UiInputMethod]),
        )];
        // the compositor's parser (the same `uid = "names"` format)
        for line in render(&apps).lines().filter(|l| !l.starts_with('#')) {
            let (uid, names) = line.split_once(" = ").unwrap();
            assert_eq!(uid, "20005");
            assert_eq!(names, "\"ui.input-method,ui.system-dialog\"");
        }
        let dir = std::env::temp_dir().join(format!("ulaunch-rights-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write(&dir.join("apps.toml"), &apps).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("apps.toml")).unwrap(),
            render(&apps)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
