//! Large read-only files of a disk (language models are tens of gigabytes): opened as a sparse
//! memory object whose pages are read from the disk when they are first read or mapped, instead
//! of the whole file being read into kernel memory at `open`. A program that maps such a file
//! shares these pages – the file is in memory once.
//!
//! Applies to regular ext4 files from [`MIN_BYTES`] on that are opened for reading only.

use crate::memfd::MemObj;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use spin::Mutex;

/// From this size on a file is not read as a whole.
pub const MIN_BYTES: u64 = 64 << 20;
const PAGE: u64 = 4096;

/// The open large files by host path: a second `open` shares the pages already read.
static OPEN: Mutex<BTreeMap<String, Weak<MemObj>>> = Mutex::new(BTreeMap::new());

/// Reads `pages` file pages from page `first` on; parts the file has no blocks for are zero.
fn read(mount: usize, runs: &[(u64, u64, u64)], first: u64, pages: usize) -> Option<Vec<u8>> {
    let mut out = crate::mem::try_vec(pages * PAGE as usize)?;
    let end = first + pages as u64;
    for &(file_block, disk_block, len) in runs.iter().filter(|r| r.0 < end && r.0 + r.2 > first) {
        let (from, to) = (file_block.max(first), (file_block + len).min(end));
        let data = crate::fs::with_ext4(mount, |x| x.blocks(disk_block + (from - file_block), to - from))??;
        let at = ((from - first) * PAGE) as usize;
        let n = data.len().min(out.len() - at);
        out[at..at + n].copy_from_slice(&data[..n]);
    }
    Some(out)
}

/// The memory object for the large file at host path `path`; `None`: not such a file (the
/// caller opens it the ordinary way).
pub fn open(path: &str) -> Option<Arc<MemObj>> {
    if crate::fs::ram_has(path) {
        return None;
    }
    if let Some(obj) = OPEN.lock().get(path).and_then(Weak::upgrade) {
        return Some(obj);
    }
    let (mount, rel) = crate::fs::ext4_at(path)?;
    let layout = crate::fs::with_ext4(mount, |x| x.layout(&rel, MIN_BYTES))??;
    let runs = layout.runs;
    let obj = Arc::new(MemObj::from_source(layout.size, alloc::boxed::Box::new(move |first, pages| read(mount, &runs, first, pages)))?);
    let mut open = OPEN.lock();
    open.retain(|_, o| o.strong_count() > 0);
    open.insert(String::from(path), Arc::downgrade(&obj));
    Some(obj)
}
