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
/// Directories of the initial namespace that show the kernel's /proc (proc mounted there by a
/// process holding the mount right; the initial table itself never changes).
static INITIAL_PROC: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// At most this many such directories.
const INITIAL_PROC_MAX: usize = 64;

/// proc mounted on (`on`) or unmounted from the directory `at` of the initial namespace.
pub fn initial_proc(at: &str, on: bool) {
    if at == "/proc" || below(at, "/proc").is_some() {
        return;
    }
    let mut l = INITIAL_PROC.lock();
    if on {
        if !l.iter().any(|d| d == at) && l.len() < INITIAL_PROC_MAX && l.try_reserve(1).is_ok() {
            l.push(at.to_string());
        }
    } else {
        l.retain(|d| d != at);
    }
}

/// The kernel's /proc path for a path of the initial namespace below such a directory.
fn initial_proc_path(path: &str) -> Option<String> {
    let l = INITIAL_PROC.lock();
    l.iter().find_map(|d| below(path, d).map(|rest| join("/proc", rest)))
}

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

/// Per-mount flags the kernel enforces.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Opts {
    /// Nothing may be written below the mount.
    pub read_only: bool,
    /// Nothing below the mount may be executed or mapped executable (`noexec`).
    pub no_exec: bool,
}

impl Opts {
    /// The stricter of two flag sets (a bind keeps what its source forbids).
    pub fn union(self, other: Opts) -> Opts {
        Opts { read_only: self.read_only || other.read_only, no_exec: self.no_exec || other.no_exec }
    }
}

/// One entry of a mount table.
#[derive(Clone)]
pub struct Mount {
    at: String,
    source: Source,
    opts: Opts,
    fstype: &'static str,
}

/// Result of translating a namespace path.
pub struct Target {
    /// Host path the namespace path stands for.
    pub host: String,
    /// Flags of the covering mount.
    pub opts: Opts,
    /// The path is the mount point itself (a directory or, for a bind of a single file, a file).
    pub mount_point: bool,
    /// The covering mount shows the whole host tree (the initial root, or a bind of it).
    pub whole_host: bool,
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
                    let root = Mount { at: String::from("/"), source: Source::Tree(String::from("/")), opts: Opts::default(), fstype: "rootfs" };
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
        if self.initial {
            if let Some(host) = initial_proc_path(path) {
                return Target { host, opts: Opts::default(), mount_point: false, whole_host: false };
            }
        }
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
            Some((m, rest)) => {
                let root = m.source.host_root();
                Target { host: join(&root, rest), opts: m.opts, mount_point: rest.is_empty(), whole_host: root == "/" }
            }
            // unreachable: every table holds "/", kept as a closed door rather than a panic
            None => Target { host: String::from("/\0tmpfs"), opts: Opts { read_only: true, no_exec: true }, mount_point: false, whole_host: false },
        }
    }

    /// Namespace paths that may show the host path `host`, most specific mount first (to be checked
    /// with [`MountNs::translate`]: a later mount can cover them).
    pub fn paths_of(&self, host: &str) -> Vec<String> {
        let mounts = self.mounts.lock();
        let mut found: Vec<(usize, String)> = mounts
            .iter()
            .filter_map(|m| {
                let root = m.source.host_root();
                below(host, &root).map(|rest| (root.len(), join(&m.at, rest)))
            })
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0));
        found.into_iter().map(|f| f.1).collect()
    }

    /// What a bind mount of the namespace path `path` shows: the covering mount's source, narrowed to
    /// `path`, and that mount's flags.
    pub fn source_of(&self, path: &str) -> (Source, Opts) {
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
                (src, m.opts)
            }
            None => (Source::Tree(String::from("/\0tmpfs")), Opts { read_only: true, no_exec: true }),
        }
    }

    /// Whether the host entry `host` is the cell's own: reachable below the root `root` through a
    /// writable mount of this namespace that does not show the whole host tree (what the right
    /// `CELL_CHOWN` may change).
    pub fn owned_by_cell(&self, host: &str, root: &str) -> bool {
        self.writable_in_view(host, root, false)
    }

    /// Whether the host entry `host` can be changed through the view with root `root`: it is visible at
    /// some path of the view, and at that path the visible mount (not one hidden below a later mount)
    /// is writable; with `whole_host` false that mount must not show the whole host tree. Checked from
    /// the host side, so it also holds for a descriptor (`fchmod`, `fchown`) that was opened through a
    /// read-only mount or came from outside the view.
    pub fn writable_in_view(&self, host: &str, root: &str, whole_host: bool) -> bool {
        let candidates: Vec<String> = {
            let mounts = self.mounts.lock();
            mounts
                .iter()
                .filter(|m| !m.opts.read_only && below(&m.at, root).is_some())
                .filter_map(|m| {
                    let src = m.source.host_root();
                    if src == "/" && !whole_host {
                        return None;
                    }
                    below(host, &src).map(|rest| join(&m.at, rest))
                })
                .collect()
        };
        candidates.iter().any(|at| {
            let t = self.translate(at);
            t.host == host && !t.opts.read_only && (whole_host || !t.whole_host)
        })
    }

    /// Add a mount at the namespace path `at`.
    pub fn add(&self, at: &str, source: Source, opts: Opts, fstype: &'static str) -> Result<(), MountError> {
        let mut mounts = self.mounts.lock();
        if mounts.len() >= MAX_MOUNTS {
            return Err(MountError::Full);
        }
        mounts.push(Mount { at: at.to_string(), source, opts, fstype });
        Ok(())
    }

    /// Replace the flags of the topmost mount at `at` (as Linux `MS_REMOUNT` does).
    pub fn remount(&self, at: &str, opts: Opts) -> Result<(), MountError> {
        let mut mounts = self.mounts.lock();
        let m = mounts.iter_mut().rev().find(|m| m.at == at).ok_or(MountError::NotMountPoint)?;
        m.opts = opts;
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

    /// The table for `/proc/mounts`: (mount point, file system type, flags).
    pub fn table(&self) -> Vec<(String, &'static str, Opts)> {
        self.mounts.lock().iter().map(|m| (m.at.clone(), m.fstype, m.opts)).collect()
    }
}
