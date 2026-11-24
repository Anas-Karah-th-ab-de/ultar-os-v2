//! Path resolution inside a view: every path of a process is resolved here, in view coordinates,
//! and only the result is translated to a host path.
//!
//! `..` is clamped at the view root and symbolic links are followed in view coordinates (an absolute
//! target restarts at the view root), so no input can name a host path outside the view.

use super::mount::{join, MountNs};
use super::{tmpfs, Isolation, ViewTag};
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Most symbolic links followed in one resolution.
pub const MAX_LINKS: u32 = 40;

/// Why a path could not be resolved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathError {
    /// Too many symbolic links (a loop).
    TooManyLinks,
    /// A directory descriptor from another view was used as the base.
    OtherView,
}

impl PathError {
    /// Linux error number (negative): `ELOOP` or `EXDEV`.
    pub fn errno(self) -> i64 {
        match self {
            PathError::TooManyLinks => -40,
            PathError::OtherView => -18,
        }
    }
}

/// Snapshot of a group's file tree view, taken under the process lock and used without it.
pub struct View {
    mnt: Arc<MountNs>,
    root: String,
    confined: bool,
    tag: ViewTag,
}

/// A resolved path.
pub struct Loc {
    /// Path as the process sees it (absolute, normalised, links resolved).
    pub view: String,
    /// Path in the kernel's host tree.
    pub host: String,
    /// The covering mount is read-only.
    pub read_only: bool,
    /// The view is narrower than the host tree.
    pub confined: bool,
    /// Tag of the view the path was resolved in.
    pub tag: ViewTag,
}

impl View {
    /// Snapshot the view of a group.
    pub fn of(iso: &Isolation) -> View {
        View {
            mnt: iso.mnt.clone(),
            root: iso.root.clone(),
            confined: iso.confined(),
            tag: iso.view_tag(),
        }
    }

    /// Tag of this view.
    pub fn tag(&self) -> ViewTag {
        self.tag
    }

    fn locate(&self, parts: &[String]) -> Loc {
        let mut view = String::from("/");
        view.push_str(&parts.join("/"));
        let t = self.mnt.translate(&join(&self.root, &view));
        Loc {
            view,
            host: t.host,
            read_only: t.read_only,
            confined: self.confined,
            tag: self.tag,
        }
    }

    /// Host path of the absolute, already resolved view path `view_path`.
    pub fn host_of(&self, view_path: &str) -> String {
        self.mnt.translate(&join(&self.root, view_path)).host
    }

    /// Resolve `path` relative to the view directory `base`. With `follow_last` a symbolic link in the
    /// last component is followed too.
    pub fn walk(&self, base: &str, path: &str, follow_last: bool) -> Result<Loc, PathError> {
        let mut parts: Vec<String> = Vec::new();
        if !path.starts_with('/') {
            parts.extend(base.split('/').filter(|c| !c.is_empty()).map(String::from));
        }
        let mut todo: Vec<String> = path.split('/').rev().map(String::from).collect();
        let mut links = 0;
        while let Some(c) = todo.pop() {
            match c.as_str() {
                "" | "." => continue,
                ".." => {
                    parts.pop();
                    continue;
                }
                _ => parts.push(c),
            }
            // the unconfined host view has no links the walker must resolve: purely lexical, as before
            if !self.confined {
                continue;
            }
            if todo.iter().all(|c| c.is_empty() || c == ".") && !follow_last {
                break;
            }
            let here = self.locate(&parts);
            if let Some(target) = link_at(&here) {
                links += 1;
                if links > MAX_LINKS {
                    return Err(PathError::TooManyLinks);
                }
                parts.pop();
                if target.starts_with('/') {
                    parts.clear();
                }
                todo.extend(target.split('/').rev().map(String::from));
            }
        }
        Ok(self.locate(&parts))
    }
}

/// Target of the symbolic link at a resolved location, without following it.
pub fn link_at(loc: &Loc) -> Option<String> {
    // the device tree's `driver` links (`devices/sysfs.rs`)
    if loc.host.starts_with("/sys/bus/") {
        return crate::devices::sysfs::link(&loc.host);
    }
    if tmpfs::is_private(&loc.host) {
        return tmpfs::readlink(&loc.host);
    }
    // The unconfined host view keeps the old behaviour: links exist only on ext4 and ext4 follows them itself.
    // A confined view must not leave that to ext4 (it would resolve absolute targets against the disk's root).
    if loc.confined {
        return crate::fs::disk_link(&loc.host);
    }
    None
}

/// Parent directory of an absolute normalised path.
pub fn parent(path: &str) -> String {
    match path.rfind('/') {
        Some(0) | None => String::from("/"),
        Some(i) => path[..i].to_string(),
    }
}
