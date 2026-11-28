//! After a package is installed: its program entries become apps – with their program
//! resolved inside the Linux area and their symbol copied for the surfaces.
//!
//! Everything read here lies below the Linux area, which an installation (and with it a
//! package's scripts) could have changed: every path is resolved without following a
//! link out of the area, files are read with a size limit.

use crate::desktop::{parse_entry, Entry};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Largest entry or file list read.
const MAX_READ: u64 = 1 << 20;
/// Largest symbol copied.
const MAX_ICON: u64 = 1 << 20;
/// Most programs one package becomes.
pub const MAX_ENTRIES: usize = 16;
/// Where programs are looked for when an entry names no absolute path.
const SEARCH: [&str; 4] = ["/usr/bin", "/usr/games", "/bin", "/usr/local/bin"];
/// Sizes of symbols looked for, best first.
const ICON_SIZES: [&str; 7] = [
    "128x128", "96x96", "64x64", "256x256", "48x48", "512x512", "scalable",
];

/// Reads at most [`MAX_READ`] bytes of the file at `path` (no link followed at the end).
fn read_small(path: &Path) -> Option<String> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() {
        return None;
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_READ)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

/// `inside` (absolute, as the cell sees it) below `runtime`: only plain names, and the
/// file the host reaches through every link on the way must still lie inside the area
/// (an absolute link of the area points at the host's own tree and is refused).
pub fn host_path(runtime: &Path, inside: &str) -> Option<PathBuf> {
    let rel = Path::new(inside).strip_prefix("/").ok()?;
    if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        return None;
    }
    let base = std::fs::canonicalize(runtime).ok()?;
    let real = std::fs::canonicalize(runtime.join(rel)).ok()?;
    real.starts_with(&base).then_some(real)
}

/// The files a package installed (`/var/lib/dpkg/info/<package>[:<arch>].list`).
pub fn package_files(runtime: &Path, package: &str) -> Vec<String> {
    for name in [
        format!("{package}.list"),
        format!("{package}:amd64.list"),
        format!("{package}:arm64.list"),
    ] {
        if let Some(text) = read_small(&runtime.join("var/lib/dpkg/info").join(name)) {
            return text.lines().map(str::to_owned).collect();
        }
    }
    Vec::new()
}

/// The program `exec` names, as an absolute path inside the area that is an executable
/// file there.
fn resolve_program(runtime: &Path, exec: &str) -> Option<String> {
    let candidates: Vec<String> = if exec.starts_with('/') {
        vec![exec.to_owned()]
    } else if exec.contains('/') {
        return None;
    } else {
        SEARCH.iter().map(|d| format!("{d}/{exec}")).collect()
    };
    candidates.into_iter().find(|c| {
        use std::os::unix::fs::PermissionsExt;
        host_path(runtime, c)
            .and_then(|p| std::fs::metadata(p).ok())
            .is_some_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    })
}

/// A symbol for `icon`: an absolute path, or a name in the hicolor theme or `pixmaps`.
fn find_icon(runtime: &Path, icon: &str) -> Option<PathBuf> {
    if icon.is_empty() || icon.contains("..") {
        return None;
    }
    let mut candidates = Vec::new();
    if icon.starts_with('/') {
        candidates.push(icon.to_owned());
    } else if !icon.contains('/') {
        for size in ICON_SIZES {
            for ext in ["png", "svg"] {
                candidates.push(format!("/usr/share/icons/hicolor/{size}/apps/{icon}.{ext}"));
            }
        }
        candidates.push(format!("/usr/share/pixmaps/{icon}.png"));
        candidates.push(format!("/usr/share/pixmaps/{icon}.svg"));
    }
    candidates.iter().find_map(|c| {
        let path = host_path(runtime, c)?;
        let meta = std::fs::symlink_metadata(&path).ok()?;
        (meta.is_file() && meta.len() <= MAX_ICON).then_some(path)
    })
}

/// One program of a package, ready to become an app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// Its entry.
    pub entry: Entry,
    /// Its program inside the area.
    pub exec: String,
    /// Its symbol on the host (below the area), if it has one.
    pub icon: Option<PathBuf>,
}

/// The programs `package` brought: visible application entries below
/// `/usr/share/applications` whose program exists in the area.
pub fn programs(runtime: &Path, package: &str) -> Vec<Program> {
    let mut out = Vec::new();
    for file in package_files(runtime, package) {
        let Some(name) = file
            .strip_prefix("/usr/share/applications/")
            .and_then(|n| n.strip_suffix(".desktop"))
            .filter(|n| !n.contains('/'))
        else {
            continue;
        };
        let Some(text) = host_path(runtime, &file).and_then(|p| read_small(&p)) else {
            continue;
        };
        let Some(entry) = parse_entry(name, &text) else {
            continue;
        };
        let Some(exec) = resolve_program(runtime, &entry.exec) else {
            continue;
        };
        let icon = find_icon(runtime, &entry.icon);
        out.push(Program { entry, exec, icon });
        if out.len() == MAX_ENTRIES {
            break;
        }
    }
    out
}

/// Copies the symbol of an app to `<icons>/<id>.<ext>` (`0644`); the path, or empty.
pub fn copy_icon(icon: Option<&Path>, icons: &Path, id: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let Some(source) = icon else {
        return String::new();
    };
    let ext = if source.extension().is_some_and(|e| e == "svg") {
        "svg"
    } else {
        "png"
    };
    let target = icons.join(format!("{id}.{ext}"));
    let Ok(mut from) = std::fs::File::open(source) else {
        return String::new();
    };
    let mut bytes = Vec::new();
    if (&mut from).take(MAX_ICON).read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    let tmp = icons.join(format!(".{id}.tmp"));
    let written = std::fs::write(&tmp, &bytes)
        .and_then(|()| std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644)))
        .and_then(|()| std::fs::rename(&tmp, &target));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return String::new();
    }
    target.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn area(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("ufh-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in [
            "usr/bin",
            "usr/share/applications",
            "usr/share/icons/hicolor/64x64/apps",
            "var/lib/dpkg/info",
        ] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::os::unix::fs::symlink("usr/bin", root.join("bin")).unwrap();
        root
    }

    #[test]
    fn a_package_becomes_programs() {
        let r = area("ok");
        let bin = r.join("usr/bin/mousepad");
        std::fs::write(&bin, b"#!").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            r.join("usr/share/applications/org.xfce.mousepad.desktop"),
            "[Desktop Entry]\nType=Application\nName=Mousepad\nExec=mousepad %U\nIcon=mp\n",
        )
        .unwrap();
        std::fs::write(r.join("usr/share/icons/hicolor/64x64/apps/mp.png"), b"png").unwrap();
        std::fs::write(
            r.join("var/lib/dpkg/info/mousepad:amd64.list"),
            "/usr/bin/mousepad\n/usr/share/applications/org.xfce.mousepad.desktop\n/etc/passwd\n",
        )
        .unwrap();
        let p = programs(&r, "mousepad");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].exec, "/usr/bin/mousepad");
        assert!(p[0].icon.as_ref().unwrap().ends_with("mp.png"));
        let icons = r.join("icons");
        std::fs::create_dir_all(&icons).unwrap();
        let copied = copy_icon(p[0].icon.as_deref(), &icons, "foreign.deb-mousepad");
        assert!(copied.ends_with("foreign.deb-mousepad.png"));
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn links_out_of_the_area_are_not_followed() {
        let r = area("links");
        std::os::unix::fs::symlink("/etc", r.join("usr/share/escape")).unwrap();
        assert_eq!(host_path(&r, "/usr/share/escape/passwd"), None);
        assert_eq!(host_path(&r, "/usr/../etc"), None);
        assert_eq!(
            host_path(&r, "/bin"),
            Some(std::fs::canonicalize(r.join("usr/bin")).unwrap())
        );
        assert_eq!(resolve_program(&r, "../../bin/sh"), None);
        let _ = std::fs::remove_dir_all(&r);
    }
}
