//! Mount namespaces: the per-namespace table that maps namespace paths to the kernel's host tree.

use super::tmpfs::Tmpfs;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

/// Most mounts one namespace may hold.
pub const MAX_MOUNTS: usize = 256;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static INITIAL: spin::Once<Arc<MountNs>> = spin::Once::new();

/// Why a change of the mount table was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MountError {
    /// The path is not a mount point of this namespace.
    NotMountPoint,
    /// Other mounts sit below the mount point.
    Busy,
    /// The namespace already holds `MAX_MOUNTS` mounts.
    Full,
    /// The arguments do not describe a valid operation (e.g. unmounting `/`).
    Invalid,
}

impl MountError {
    /// Linux error number (negative) for this refusal.
    pub fn errno(self) -> i64 {
        match self {
            MountError::NotMountPoint | MountError::Invalid => -22,
            MountError::Busy => -16,
            MountError::Full => -28,
        }
    }
}

/// What a mount shows.
#[derive(Clone)]
pub enum Source {
    /// A subtree of the host tree (bind mounts, proc, devtmpfs, the initial root).
    Tree(String),
    /// A directory (path relative to the tree's root, "" for the root itself) of a private RAM tree.
    Tmpfs(Arc<Tmpfs>, String),
}

impl Source {
    fn host_root(&self) -> String {
        match self {
            Source::Tree(p) => p.clone(),
            Source::Tmpfs(t, sub) => join(&t.host_root(), sub),
        }
    }
}

/// One entry of a mount table.
#[derive(Clone)]
pub struct Mount {
    at: String,
    source: Source,
    read_only: bool,
    fstype: &'static str,
}

/// Result of translating a namespace path.
pub struct Target {
    /// Host path the namespace path stands for.
    pub host: String,
    /// Whether the covering mount is read-only.
    pub read_only: bool,
}

/// One mount namespace.
pub struct MountNs {
    id: u64,
    initial: bool,
    epoch: AtomicU64,
    mounts: Mutex<Vec<Mount>>,
}

/// Rest of `path` below `prefix` ("" when equal), or `None` if `path` is not at or below `prefix`.
pub fn below<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix == "/" {
        return Some(if path == "/" { "" } else { path });
    }
    let rest = path.strip_prefix(prefix)?;
    (rest.is_empty() || rest.starts_with('/')).then_some(rest)
}

/// Absolute path `rest` (or "") placed below the directory `dir`.
pub fn join(dir: &str, rest: &str) -> String {
    let rest = if rest == "/" { "" } else { rest };
    match (dir, rest) {
        ("/", "") => String::from("/"),
        ("/", r) => r.to_string(),
        (d, r) => alloc::format!("{d}{r}"),
    }
}

/// Where `path` lies after `pivot_root(new_root, put_old)`; `put_rel` is `put_old` relative to `new_root`.
pub fn repath(path: &str, new_root: &str, put_rel: &str) -> String {
    match below(path, new_root) {
        Some(rest) => join("/", rest),
        None => join(put_rel, path),
    }
}

impl MountNs {
    /// The initial namespace: the host tree itself. Its table never changes.
    pub fn initial() -> Arc<MountNs> {
        INITIAL
            .call_once(|| {
                // lives as long as the machine: it belongs to the kernel, not to whoever asks first
                crate::heap::kernel(|| {
                    let root = Mount { at: String::from("/"), source: Source::Tree(String::from("/")), read_only: false, fstype: "rootfs" };
                    Arc::new(MountNs { id: NEXT_ID.fetch_add(1, Ordering::Relaxed), initial: true, epoch: AtomicU64::new(0), mounts: Mutex::new(vec![root]) })
                })
            })
            .clone()
    }

    /// New namespace with a copy of this one's table.
    pub fn copy(&self) -> Arc<MountNs> {
        let mounts = self.mounts.lock().clone();
        Arc::new(MountNs { id: NEXT_ID.fetch_add(1, Ordering::Relaxed), initial: false, epoch: AtomicU64::new(0), mounts: Mutex::new(mounts) })
    }

    /// Identity of the namespace (shown by `/proc/<pid>/ns/mnt`).
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Whether this is the initial namespace.
    pub fn is_initial(&self) -> bool {
        self.initial
    }

    /// Counter that changes whenever `pivot_root` rearranged the namespace.
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }

    /// Host path for the namespace path `path` (absolute, normalised).
    pub fn translate(&self, path: &str) -> Target {
        let mounts = self.mounts.lock();
        let mut best: Option<(&Mount, &str)> = None;
        for m in mounts.iter() {
            if let Some(rest) = below(path, &m.at) {
                // the longest mount point wins; of two equal ones the later (upper) mount
                if best.map(|b| m.at.len() >= b.0.at.len()).unwrap_or(true) {
                    best = Some((m, rest));
                }
            }
        }
        match best {
            Some((m, rest)) => Target { host: join(&m.source.host_root(), rest), read_only: m.read_only },
            // unreachable: every table holds "/", kept as a closed door rather than a panic
            None => Target { host: String::from("/\0tmpfs"), read_only: true },
        }
    }

    /// What a bind mount of the namespace path `path` shows: the covering mount's source, narrowed to
    /// `path`, and whether that mount is read-only.
    pub fn source_of(&self, path: &str) -> (Source, bool) {
        let mounts = self.mounts.lock();
        let mut best: Option<(&Mount, &str)> = None;
        for m in mounts.iter() {
            if let Some(rest) = below(path, &m.at) {
                if best.map(|b| m.at.len() >= b.0.at.len()).unwrap_or(true) {
                    best = Some((m, rest));
                }
            }
        }
        match best {
            Some((m, rest)) => {
                let src = match &m.source {
                    Source::Tree(p) => Source::Tree(join(p, rest)),
                    Source::Tmpfs(t, sub) => Source::Tmpfs(t.clone(), alloc::format!("{sub}{rest}")),
                };
                (src, m.read_only)
            }
            None => (Source::Tree(String::from("/\0tmpfs")), true),
        }
    }

    /// Add a mount at the namespace path `at`.
    pub fn add(&self, at: &str, source: Source, read_only: bool, fstype: &'static str) -> Result<(), MountError> {
        let mut mounts = self.mounts.lock();
        if mounts.len() >= MAX_MOUNTS {
            return Err(MountError::Full);
        }
        mounts.push(Mount { at: at.to_string(), source, read_only, fstype });
        Ok(())
    }

    /// Change the read-only flag of the topmost mount at `at`.
    pub fn remount(&self, at: &str, read_only: bool) -> Result<(), MountError> {
        let mut mounts = self.mounts.lock();
        let m = mounts.iter_mut().rev().find(|m| m.at == at).ok_or(MountError::NotMountPoint)?;
        m.read_only = read_only;
        Ok(())
    }

    /// Remove the topmost mount at `at`; with `detach` also every mount below it.
    pub fn remove(&self, at: &str, detach: bool) -> Result<(), MountError> {
        if at == "/" {
            return Err(MountError::Invalid);
        }
        let removed: Vec<Mount>;
        {
            let mut mounts = self.mounts.lock();
            let idx = mounts.iter().rposition(|m| m.at == at).ok_or(MountError::NotMountPoint)?;
            let has_children = mounts.iter().any(|m| m.at != at && below(&m.at, at).is_some());
            if detach {
                let (gone, kept): (Vec<Mount>, Vec<Mount>) = mounts.drain(..).partition(|m| below(&m.at, at).is_some());
                *mounts = kept;
                removed = gone;
            } else if has_children {
                return Err(MountError::Busy);
            } else {
                removed = vec![mounts.remove(idx)];
            }
        }
        // a tmpfs frees its files here, outside the table lock
        drop(removed);
        Ok(())
    }

    /// Make the mount at `new_root` the root; everything else moves below `put_old` (a path under `new_root`).
    pub fn pivot(&self, new_root: &str, put_old: &str) -> Result<(), MountError> {
        let put_rel = match below(put_old, new_root) {
            Some(r) if !r.is_empty() && new_root != "/" => r.to_string(),
            _ => return Err(MountError::Invalid),
        };
        let mut mounts = self.mounts.lock();
        if !mounts.iter().any(|m| m.at == new_root) {
            return Err(MountError::NotMountPoint);
        }
        // old-tree mounts first, so mounts of the new tree stay on top where both name the same path
        let (mut new_tree, mut old_tree): (Vec<Mount>, Vec<Mount>) = mounts.drain(..).partition(|m| below(&m.at, new_root).is_some());
        for m in new_tree.iter_mut().chain(old_tree.iter_mut()) {
            m.at = repath(&m.at, new_root, &put_rel);
        }
        // order inside the new tree must keep "/" first for equal-length ties
        old_tree.append(&mut new_tree);
        *mounts = old_tree;
        self.epoch.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// The table for `/proc/mounts`: (mount point, file system type, read-only).
    pub fn table(&self) -> Vec<(String, &'static str, bool)> {
        self.mounts.lock().iter().map(|m| (m.at.clone(), m.fstype, m.read_only)).collect()
    }
}
