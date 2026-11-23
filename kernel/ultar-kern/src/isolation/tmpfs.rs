//! Private RAM file trees for mounts inside a namespace (tmpfs), including symbolic links, and the
//! quota of every RAM tree.
//!
//! A tmpfs lives in the kernel's RAM layer under a host prefix that contains a NUL byte. No path that
//! arrives from a C string can contain NUL, so the tree is reachable only through a mount that points
//! at it. The tree is freed when the last mount of it disappears.
//!
//! **Quota.** Every tree – each tmpfs and the host's RAM layer (tree 0: files written outside a
//! writable disk) – may hold at most `quota` bytes: by default a quarter of the machine's memory
//! ([`default_quota`]), for a tmpfs whatever `size=` said at mount. Counted per entry: its data
//! (for a symbolic link the target), its path and [`ENTRY_COST`] bytes, so that empty files,
//! directories and links are not free. A descriptor that grows a file reserves the room when it
//! writes ([`grow`]), so the writer gets `ENOSPC` at `write`, not a silent loss at `close`.
//! The bytes themselves are also charged to the resource group of the writing process (`heap.rs`).

use crate::fs::{self, Node, RAM};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

const PREFIX: &str = "/\0tmpfs/";
/// Bytes counted for every entry on top of its path and data.
pub const ENTRY_COST: u64 = 128;
/// Id of the host's RAM layer in the quota table.
const HOST: u64 = 0;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Space used in one tree.
struct Usage {
    used: u64,
    quota: u64,
    /// Bytes counted per path: the larger of the stored size and what an open descriptor reserved.
    counted: BTreeMap<String, u64>,
}

/// Trees that exist (a tmpfs disappears from here when it is freed; the host tree is created on
/// first use). Lock order: `TREES` → `RAM` / `LINKS`.
static TREES: Mutex<BTreeMap<u64, Usage>> = Mutex::new(BTreeMap::new());
/// Symbolic links: host path → target as written by the creator.
static LINKS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// Quota of a tree where nothing else is configured: a quarter of the machine's memory, at least 8 MiB.
pub fn default_quota() -> u64 {
    (crate::mem::total_bytes() / 4).max(8 << 20)
}

/// Parse the `size=` option of a tmpfs mount (`data` argument of `mount`): bytes with an optional
/// `k`/`m`/`g` suffix or a percentage of the machine's memory. `Ok(None)`: no such option.
/// `Err(())`: malformed.
pub fn parse_size(options: &str) -> Result<Option<u64>, ()> {
    let mut size = None;
    for opt in options.split(',') {
        let Some(v) = opt.trim().strip_prefix("size=") else { continue };
        let (digits, unit): (&str, u64) = match v.as_bytes().last().map(|b| b.to_ascii_lowercase()) {
            Some(b'k') => (&v[..v.len() - 1], 1 << 10),
            Some(b'm') => (&v[..v.len() - 1], 1 << 20),
            Some(b'g') => (&v[..v.len() - 1], 1 << 30),
            Some(b'%') => (&v[..v.len() - 1], 0),
            _ => (v, 1),
        };
        let n: u64 = digits.parse().map_err(|_| ())?;
        size = Some(if unit == 0 {
            if n > 100 {
                return Err(());
            }
            crate::mem::total_bytes() / 100 * n
        } else {
            n.checked_mul(unit).ok_or(())?
        });
    }
    Ok(size)
}

/// `mode=<octal>`, `uid=<n>`, `gid=<n>` among the options of a tmpfs mount: permission bits, owner
/// and group of the tree's root directory. Without them the root is `0755`, owned by root (stricter
/// than Linux' `1777`: nobody but root writes there unless the mounter says so).
/// `Err(())`: malformed.
pub fn parse_root_attr(options: &str) -> Result<(u32, u32, u32), ()> {
    let (mut mode, mut uid, mut gid) = (0o755, 0, 0);
    for opt in options.split(',') {
        let opt = opt.trim();
        if let Some(v) = opt.strip_prefix("mode=") {
            mode = u32::from_str_radix(v, 8).ok().filter(|m| *m <= 0o7777).ok_or(())?;
        } else if let Some(v) = opt.strip_prefix("uid=") {
            uid = v.parse().map_err(|_| ())?;
        } else if let Some(v) = opt.strip_prefix("gid=") {
            gid = v.parse().map_err(|_| ())?;
        }
    }
    Ok((mode, uid, gid))
}

/// One private RAM tree. Dropping the last reference frees its files.
pub struct Tmpfs {
    id: u64,
}

impl Tmpfs {
    /// Create an empty tree that may hold `quota` bytes (`None`: [`default_quota`]).
    pub fn create(quota: Option<u64>) -> Arc<Tmpfs> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = format!("{PREFIX}{id}/");
        let mut trees = TREES.lock();
        // the tables are shared by all trees: their nodes belong to the kernel
        crate::heap::kernel(|| {
            trees.insert(id, Usage { used: 0, quota: quota.unwrap_or_else(default_quota), counted: BTreeMap::new() });
            RAM.lock().insert(root, Arc::new(Vec::new()));
        });
        Arc::new(Tmpfs { id })
    }

    /// Host path of the tree's root directory.
    pub fn host_root(&self) -> String {
        format!("{PREFIX}{}", self.id)
    }
}

impl Drop for Tmpfs {
    fn drop(&mut self) {
        let mut trees = TREES.lock();
        trees.remove(&self.id);
        let pre = format!("{PREFIX}{}/", self.id);
        RAM.lock().retain(|k, _| !k.starts_with(&pre));
        LINKS.lock().retain(|k, _| !k.starts_with(&pre));
        crate::perm::forget_tree(&self.host_root());
    }
}

/// Whether the host path lies in the private area (inside some tmpfs or its bookkeeping prefix).
pub fn is_private(path: &str) -> bool {
    path.starts_with(PREFIX) || path == "/\0tmpfs"
}

fn tree_id(path: &str) -> Option<u64> {
    path.strip_prefix(PREFIX)?.split('/').next()?.parse().ok()
}

/// Tree a host path belongs to for the quota: its tmpfs, else the host's RAM layer.
fn quota_tree(path: &str) -> u64 {
    tree_id(path).unwrap_or(HOST)
}

fn cost(path: &str, data: u64) -> u64 {
    ENTRY_COST + path.len() as u64 + data
}

/// Run `f` on the usage of the tree that holds `path`; `None` if that tmpfs no longer exists.
fn with_tree<R>(path: &str, f: impl FnOnce(&mut Usage) -> R) -> Option<R> {
    let id = quota_tree(path);
    let mut trees = TREES.lock();
    if id == HOST && !trees.contains_key(&HOST) {
        crate::heap::kernel(|| trees.insert(HOST, Usage { used: 0, quota: default_quota(), counted: BTreeMap::new() }));
    }
    trees.get_mut(&id).map(f)
}

impl Usage {
    /// Count `path` with `data` bytes. `shrink`: the count may go down (the stored size is known);
    /// otherwise it only grows (a reservation). `false`: over quota, nothing changed.
    fn count(&mut self, path: &str, data: u64, shrink: bool) -> bool {
        let new = cost(path, data);
        let old = self.counted.get(path).copied();
        let new = if shrink { new } else { new.max(old.unwrap_or(0)) };
        let old_bytes = old.unwrap_or(0);
        if new > old_bytes && self.used - old_bytes + new > self.quota {
            // the running system call reports "no space" whatever the file layer's yes/no becomes
            crate::percpu::me().errno = 28;
            return false;
        }
        match self.counted.get_mut(path) {
            Some(c) => *c = new,
            None => {
                // the entry of the host tree is kept by the kernel; a tmpfs' own table is charged
                // to whoever fills it and goes with the tree
                let key = path.to_string();
                if quota_tree(path) == HOST {
                    crate::heap::kernel(|| self.counted.insert(key, new));
                } else {
                    self.counted.insert(key, new);
                }
            }
        }
        self.used = self.used - old_bytes + new;
        true
    }

    fn forget(&mut self, path: &str) {
        if let Some(c) = self.counted.remove(path) {
            self.used -= c;
        }
    }
}

/// Reserve room for the file at `path` to hold `len` bytes. `false`: the quota of its tree does not
/// allow it (or the tmpfs is gone).
pub fn grow(path: &str, len: u64) -> bool {
    with_tree(path, |u| u.count(path, len, false)).unwrap_or(false)
}

/// Used and allowed bytes of the tree that holds `path`.
pub fn usage(path: &str) -> Option<(u64, u64)> {
    with_tree(path, |u| (u.used, u.quota))
}

/// Put `data` under `key` in the RAM layer if the quota of its tree allows it. Used for files and
/// (with a trailing `/` in `key`) directories, of a tmpfs and of the host's RAM layer alike.
pub fn store(key: &str, data: Arc<Vec<u8>>) -> bool {
    with_tree(key, |u| {
        if !u.count(key, data.len() as u64, true) {
            return false;
        }
        let k = key.to_string();
        crate::heap::kernel(|| RAM.lock().insert(k, data));
        true
    })
    .unwrap_or(false)
}

/// Remove `key` from the RAM layer; `false` if it was not there.
pub fn discard(key: &str) -> bool {
    with_tree(key, |u| {
        let was = RAM.lock().remove(key).is_some();
        if was {
            u.forget(key);
            crate::perm::forget(key.trim_end_matches('/'));
        }
        was
    })
    .unwrap_or(false)
}

/// Node at a private host path (links are not followed here; the path walker does that).
pub fn open(path: &str) -> Option<Node> {
    tree_id(path)?;
    if LINKS.lock().contains_key(path) {
        return None;
    }
    if let Some(v) = RAM.lock().get(path) {
        return Some(Node::File(v.clone()));
    }
    fs::ram_dir(path).map(Node::Dir)
}

/// Kind and size at a private host path: (0 file, 1 directory), size.
pub fn stat(path: &str) -> Option<(u8, u64)> {
    tree_id(path)?;
    // a directory: known without building its listing
    if !LINKS.lock().contains_key(path) && !RAM.lock().contains_key(path) && fs::ram_is_dir(path) {
        return Some((1, 0));
    }
    match open(path)? {
        Node::File(d) => Some((0, d.len() as u64)),
        _ => Some((1, 0)),
    }
}

/// Create or replace a file. Returns false if the tree is gone or its quota is exhausted.
pub fn write(path: &str, data: Arc<Vec<u8>>) -> bool {
    tree_id(path).is_some() && store(path, data)
}

/// Create a directory. Returns false if something exists there, the tree is gone or full.
pub fn mkdir(path: &str) -> bool {
    if stat(path).is_some() || readlink(path).is_some() {
        return false;
    }
    tree_id(path).is_some() && store(&format!("{path}/"), Arc::new(Vec::new()))
}

/// Remove a file, an (empty or not) directory entry or a symbolic link.
pub fn unlink(path: &str) -> bool {
    if tree_id(path).map(|id| path == format!("{PREFIX}{id}")).unwrap_or(true) {
        return false; // the root of a tree goes away only with its last mount
    }
    let link = with_tree(path, |u| {
        let was = LINKS.lock().remove(path).is_some();
        if was {
            u.forget(path);
            crate::perm::forget(path);
        }
        was
    })
    .unwrap_or(false);
    link || discard(path) || discard(&format!("{path}/"))
}

/// Create a symbolic link at `path`. Returns false if something exists there, the tree is gone or full.
pub fn symlink(path: &str, target: &str) -> bool {
    if stat(path).is_some() || tree_id(path).is_none() {
        return false;
    }
    with_tree(path, |u| {
        let mut links = LINKS.lock();
        if links.contains_key(path) || !u.count(path, target.len() as u64, true) {
            return false;
        }
        let (k, t) = (path.to_string(), target.to_string());
        crate::heap::kernel(|| links.insert(k, t));
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
