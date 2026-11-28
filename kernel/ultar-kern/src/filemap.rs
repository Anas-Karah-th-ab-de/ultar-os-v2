//! Shared mappings of files (`MAP_SHARED`, docs/integration/kern-wine.md): while any process maps a file
//! shared, the file has one memory object; every mapping (in any process) maps its pages, so a write
//! through one mapping is seen by every other mapping and by `read`, and `write` reaches the mappings.
//!
//! The file system stores whole files (fs::store_file). The object is written to the file by `msync`,
//! by `munmap` and when its last mapping goes away (also at the end of a process). A process that also
//! holds the file open gets the stored bytes into its descriptors, so their close does not put back
//! an older copy.

use crate::memfd::MemObj;
use crate::proc::{FdKind, Proc};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

/// One mapped file: its path, its object, and whether a writable mapping ever existed.
struct Entry {
    path: String,
    obj: Arc<MemObj>,
    written: bool,
}

static FILES: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

/// The object of the file `path` (made from `data`, the file's bytes, if no mapping exists yet).
/// `None`: no memory.
pub fn object(path: &str, data: &[u8], writable: bool) -> Option<Arc<MemObj>> {
    let mut l = FILES.lock();
    if let Some(e) = l.iter_mut().find(|e| e.path == path) {
        e.written |= writable;
        return Some(e.obj.clone());
    }
    let obj = Arc::new(MemObj::new(false));
    obj.set_size(data.len() as u64).ok()?;
    if !data.is_empty() {
        obj.write(0, data).ok()?;
    }
    if !crate::mem::try_reserve(&mut l, 1) {
        return None;
    }
    l.push(Entry { path: String::from(path), obj: obj.clone(), written: writable });
    Some(obj)
}

/// The object of `path` while the file is mapped shared somewhere.
pub fn live(path: &str) -> Option<Arc<MemObj>> {
    if FILES.lock().is_empty() {
        return None;
    }
    FILES.lock().iter().find(|e| e.path == path).map(|e| e.obj.clone())
}

/// The path of a file object (`None`: `obj` is a memory object of `memfd_create`, not a file).
fn path_of(obj: &Arc<MemObj>) -> Option<(String, bool)> {
    FILES.lock().iter().find(|e| Arc::ptr_eq(&e.obj, obj)).map(|e| (e.path.clone(), e.written))
}

/// The bytes of a file object as one stored file.
fn content(obj: &MemObj) -> Arc<Vec<u8>> {
    Arc::new(obj.read(0, obj.size() as usize))
}

/// Write the file objects mapped in `[va, va + len)` of `p` to their files (`msync`, `munmap`); the
/// descriptors of `p` that hold such a file take the stored bytes.
pub fn sync_range(p: &mut Proc, va: u64, len: u64) {
    if FILES.lock().is_empty() {
        return;
    }
    let objs: Vec<Arc<MemObj>> = p
        .space()
        .shared
        .lock()
        .iter()
        .filter(|r| r.0 < va + len && r.1 > va)
        .filter_map(|r| match &r.2 {
            crate::memfd::Backing::Obj(o, true, _) => Some(o.clone()),
            _ => None,
        })
        .collect();
    for o in objs {
        let Some((path, _)) = path_of(&o) else { continue };
        let bytes = content(&o);
        if crate::fs::store_file(&path, bytes.clone()) {
            refresh_fds(p, &path, &bytes);
        }
    }
}

/// Descriptors of `p` on `path` hold the stored bytes and nothing left to store.
fn refresh_fds(p: &mut Proc, path: &str, bytes: &Arc<Vec<u8>>) {
    for d in p.g.fds.iter_mut().flatten() {
        if let FdKind::File { data, path: fp, dirty, .. } = &mut d.kind {
            if fp == path {
                *data = bytes.clone();
                *dirty = false;
            }
        }
    }
}

/// A mapping of `obj` went away (`Backing` dropped). With the last mapping a written file object is
/// stored once more and forgotten.
pub fn unmapped(obj: &Arc<MemObj>) {
    if obj.map_count() != 0 || FILES.lock().is_empty() {
        return;
    }
    let taken = {
        let mut l = FILES.lock();
        l.iter().position(|e| Arc::ptr_eq(&e.obj, obj)).map(|i| l.swap_remove(i))
    };
    if let Some(e) = taken.filter(|e| e.written) {
        crate::fs::store_file(&e.path, content(&e.obj));
    }
}

/// `write` on a descriptor of a mapped file: the mappings see the bytes too.
pub fn wrote(path: &str, pos: u64, data: &[u8]) {
    if let Some(o) = live(path) {
        let end = pos + data.len() as u64;
        if end > o.size() && o.set_size(end).is_err() {
            return;
        }
        let _ = o.write(pos, data);
    }
}

/// `ftruncate` on a descriptor of a mapped file: the object takes the new size.
pub fn truncated(path: &str, len: u64) {
    if let Some(o) = live(path) {
        let _ = o.set_size(len);
    }
}
