//! Private RAM file trees for mounts inside a namespace (tmpfs), including symbolic links.
//!
//! A tmpfs lives in the kernel's RAM layer under a host prefix that contains a NUL byte. No path that
//! arrives from a C string can contain NUL, so the tree is reachable only through a mount that points
//! at it. The tree is freed when the last mount of it disappears.

use crate::fs::{self, Node, RAM};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

const PREFIX: &str = "/\0tmpfs/";

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Ids of the trees that still exist. Lock order: `LIVE` → `RAM` / `LINKS`.
static LIVE: Mutex<BTreeSet<u64>> = Mutex::new(BTreeSet::new());
/// Symbolic links: host path → target as written by the creator.
static LINKS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// One private RAM tree. Dropping the last reference frees its files.
pub struct Tmpfs {
    id: u64,
}

impl Tmpfs {
    /// Create an empty tree.
    pub fn create() -> Arc<Tmpfs> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let mut live = LIVE.lock();
        live.insert(id);
        RAM.lock().insert(format!("{PREFIX}{id}/"), Vec::new());
        Arc::new(Tmpfs { id })
    }

    /// Host path of the tree's root directory.
    pub fn host_root(&self) -> String {
        format!("{PREFIX}{}", self.id)
    }
}

impl Drop for Tmpfs {
    fn drop(&mut self) {
        let mut live = LIVE.lock();
        live.remove(&self.id);
        let pre = format!("{PREFIX}{}/", self.id);
        RAM.lock().retain(|k, _| !k.starts_with(&pre));
        LINKS.lock().retain(|k, _| !k.starts_with(&pre));
    }
}

/// Whether the host path lies in the private area (inside some tmpfs or its bookkeeping prefix).
pub fn is_private(path: &str) -> bool {
    path.starts_with(PREFIX) || path == "/\0tmpfs"
}

fn tree_id(path: &str) -> Option<u64> {
    path.strip_prefix(PREFIX)?.split('/').next()?.parse().ok()
}

/// Run `f` only while the tree that holds `path` still exists (so nothing is stored into a freed tree).
fn while_alive<R>(path: &str, f: impl FnOnce() -> R) -> Option<R> {
    let live = LIVE.lock();
    let id = tree_id(path)?;
    live.contains(&id).then(f)
}

/// Node at a private host path (links are not followed here; the path walker does that).
pub fn open(path: &str) -> Option<Node> {
    tree_id(path)?;
    if LINKS.lock().contains_key(path) {
        return None;
    }
    if let Some(v) = RAM.lock().get(path) {
        return Some(Node::File(Arc::new(v.clone())));
    }
    fs::ram_dir(path).map(Node::Dir)
}

/// Kind and size at a private host path: (0 file, 1 directory), size.
pub fn stat(path: &str) -> Option<(u8, u64)> {
    match open(path)? {
        Node::File(d) => Some((0, d.len() as u64)),
        _ => Some((1, 0)),
    }
}

/// Create or replace a file. Returns false if the tree is gone.
pub fn write(path: &str, data: Vec<u8>) -> bool {
    while_alive(path, || {
        RAM.lock().insert(path.to_string(), data);
    })
    .is_some()
}

/// Create a directory. Returns false if something exists there or the tree is gone.
pub fn mkdir(path: &str) -> bool {
    if stat(path).is_some() || readlink(path).is_some() {
        return false;
    }
    while_alive(path, || {
        RAM.lock().insert(format!("{path}/"), Vec::new());
    })
    .is_some()
}

/// Remove a file, an (empty or not) directory entry or a symbolic link.
pub fn unlink(path: &str) -> bool {
    if tree_id(path).map(|id| path == format!("{PREFIX}{id}")).unwrap_or(true) {
        return false; // the root of a tree goes away only with its last mount
    }
    if LINKS.lock().remove(path).is_some() {
        return true;
    }
    let mut ram = RAM.lock();
    ram.remove(path).is_some() || ram.remove(&format!("{path}/")).is_some()
}

/// Create a symbolic link at `path`. Returns false if something exists there or the tree is gone.
pub fn symlink(path: &str, target: &str) -> bool {
    if stat(path).is_some() {
        return false;
    }
    while_alive(path, || {
        let mut links = LINKS.lock();
        if links.contains_key(path) {
            return false;
        }
        links.insert(path.to_string(), target.to_string());
        true
    })
    .unwrap_or(false)
}

/// Target of the symbolic link at `path`, if there is one.
pub fn readlink(path: &str) -> Option<String> {
    if !is_private(path) {
        return None;
    }
    LINKS.lock().get(path).cloned()
}
