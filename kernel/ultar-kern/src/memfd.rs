//! Anonymous shared memory objects (`memfd_create`) with seals, and the shared page mappings of the
//! address spaces (`MAP_SHARED` of such an object, and of the framebuffer).
//!
//! An object owns whole zeroed pages, allocated when it grows (charged to the group of the process
//! that grows it) and freed when the last descriptor and the last mapping are gone. A mapping puts the
//! object's own pages into the page table *without* the page-ownership bit, so neither `munmap` nor the
//! end of the address space frees them; the address space keeps the object alive while it maps it
//! (`paging::Shared`). While any mapping exists the object cannot shrink (`EBUSY`): no page that a
//! process can still reach is ever given back.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

const EPERM: i64 = 1;
const ENOMEM: i64 = 12;
const EBUSY: i64 = 16;
const EINVAL: i64 = 22;
const EFBIG: i64 = 27;

/// `F_SEAL_SEAL`, `F_SEAL_SHRINK`, `F_SEAL_GROW`, `F_SEAL_WRITE`, `F_SEAL_FUTURE_WRITE`.
pub const SEAL_SEAL: u32 = 1;
pub const SEAL_SHRINK: u32 = 2;
pub const SEAL_GROW: u32 = 4;
pub const SEAL_WRITE: u32 = 8;
pub const SEAL_FUTURE_WRITE: u32 = 16;
const SEALS_ALL: u32 = 31;
/// `MFD_CLOEXEC`, `MFD_ALLOW_SEALING`.
pub const MFD_CLOEXEC: u64 = 1;
pub const MFD_ALLOW_SEALING: u64 = 2;
/// Largest object (the same bound as a file of the RAM layer).
pub const MAX_BYTES: u64 = 1 << 32;

/// Reads the content of `pages` pages from page `first` on (`None`: the read failed).
pub type Reader = alloc::boxed::Box<dyn Fn(u64, usize) -> Option<Vec<u8>> + Send + Sync>;
/// Pages a [`Reader`] is asked for at once (4 MiB).
const READ_AHEAD: usize = 1024;

/// Writes the zeroed page `p` out of the CPU's caches: it was zeroed through the kernel's
/// cached view, and a write-combining user or a non-coherent GPU must not miss those zeros nor
/// have a cache line written over its data later.
fn flush_page(p: u64) {
    for line in (p..p + 4096).step_by(64) {
        // SAFETY: `line` lies in the page just allocated, identity-mapped in the kernel.
        unsafe { core::arch::x86_64::_mm_clflush(line as *const u8) };
    }
}

/// A page of an object that has no memory yet.
const HOLE: u64 = 0;

/// One memory object. It is sparse like Linux' memory files: a page gets memory when it is first
/// written or mapped; until then it is a hole that reads as zero and costs nothing (Mesa makes
/// 1 GiB objects and uses a few megabytes of them).
pub struct MemObj {
    /// One entry per page of the size: the page's address, or [`HOLE`].
    pages: Mutex<Vec<u64>>,
    /// Where the content of a hole comes from (a file on a disk); `None`: holes are zero.
    source: Option<Reader>,
    /// Mapped write-combining, past the CPU's caches (GPU buffers the GT reads without looking
    /// into them): new pages are flushed out of the caches before anyone sees them.
    write_combining: bool,
    size: AtomicU64,
    seals: AtomicU32,
    /// Mappings of the object in any address space; `writable_maps`: those with write access.
    maps: AtomicU64,
    writable_maps: AtomicU64,
}

impl MemObj {
    /// Empty object; without `MFD_ALLOW_SEALING` it carries `F_SEAL_SEAL` (no seal can be added).
    pub fn new(sealing: bool) -> MemObj {
        MemObj {
            pages: Mutex::new(Vec::new()),
            source: None,
            write_combining: false,
            size: AtomicU64::new(0),
            seals: AtomicU32::new(if sealing { 0 } else { SEAL_SEAL }),
            maps: AtomicU64::new(0),
            writable_maps: AtomicU64::new(0),
        }
    }

    /// A read-only object of `size` bytes whose pages come from `source` when they are first
    /// read or mapped (a large file of a disk). `None`: no memory for the page list.
    pub fn from_source(size: u64, source: Reader) -> Option<MemObj> {
        let mut pages = Vec::new();
        let n = size.div_ceil(4096) as usize;
        if pages.try_reserve_exact(n).is_err() {
            return None;
        }
        pages.resize(n, HOLE);
        Some(MemObj {
            pages: Mutex::new(pages),
            source: Some(source),
            write_combining: false,
            size: AtomicU64::new(size),
            seals: AtomicU32::new(SEALS_ALL),
            maps: AtomicU64::new(0),
            writable_maps: AtomicU64::new(0),
        })
    }

    /// An empty object that is mapped write-combining (see the field).
    pub fn new_write_combining() -> MemObj {
        MemObj {
            pages: Mutex::new(Vec::new()),
            source: None,
            write_combining: true,
            size: AtomicU64::new(0),
            seals: AtomicU32::new(SEAL_SEAL),
            maps: AtomicU64::new(0),
            writable_maps: AtomicU64::new(0),
        }
    }

    /// Whether mappings of this object bypass the CPU's caches.
    pub fn write_combining(&self) -> bool {
        self.write_combining
    }

    pub fn size(&self) -> u64 {
        self.size.load(Ordering::SeqCst)
    }

    pub fn seals(&self) -> u32 {
        self.seals.load(Ordering::SeqCst)
    }

    /// Pages that hold data (for `st_blocks`).
    pub fn pages(&self) -> u64 {
        self.pages.lock().iter().filter(|p| **p != HOLE).count() as u64
    }

    /// Gives every hole in `pages[first..first + n]` a page (charged to the caller's group):
    /// zeroed, or with what the source reads. `false`: memory is out or the read failed; the
    /// pages filled so far stay.
    fn fill(&self, pages: &mut [u64], first: usize, n: usize) -> bool {
        let Some(read) = &self.source else {
            for slot in pages[first..first + n].iter_mut().filter(|p| **p == HOLE) {
                let Some(p) = crate::mem::alloc_page() else { return false };
                if self.write_combining {
                    flush_page(p);
                }
                *slot = p;
            }
            if self.write_combining {
                // SAFETY: orders the flushes before the pages are handed out.
                unsafe { core::arch::x86_64::_mm_mfence() };
            }
            return true;
        };
        let (mut i, end) = (first, first + n);
        while i < end {
            if pages[i] != HOLE {
                i += 1;
                continue;
            }
            // a run of holes, read at once
            let stop = (i..end.min(i + READ_AHEAD)).find(|k| pages[*k] != HOLE).unwrap_or(end.min(i + READ_AHEAD));
            let Some(data) = read(i as u64, stop - i).filter(|d| d.len() >= (stop - i) * 4096) else { return false };
            for (k, chunk) in (i..stop).zip(data.chunks_exact(4096)) {
                let Some(p) = crate::mem::alloc_page() else { return false };
                // SAFETY: `p` is a fresh page (identity-mapped); `chunk` is 4096 bytes.
                unsafe { core::ptr::copy_nonoverlapping(chunk.as_ptr(), p as *mut u8, 4096) };
                pages[k] = p;
            }
            i = stop;
        }
        true
    }

    /// `ftruncate`: grow with holes or shrink.
    pub fn set_size(&self, len: u64) -> Result<(), i64> {
        if len > MAX_BYTES {
            return Err(-EFBIG);
        }
        let mut pages = self.pages.lock();
        let now = self.size();
        let seals = self.seals();
        if len < now && seals & SEAL_SHRINK != 0 || len > now && seals & SEAL_GROW != 0 {
            return Err(-EPERM);
        }
        let want = len.div_ceil(4096) as usize;
        if want < pages.len() && self.maps.load(Ordering::SeqCst) > 0 {
            // a page another process can still reach is never given back
            return Err(-EBUSY);
        }
        let more = want.saturating_sub(pages.len());
        if pages.try_reserve(more).is_err() {
            return Err(-ENOMEM);
        }
        if pages.len() < want {
            pages.resize(want, HOLE);
        }
        while pages.len() > want {
            if let Some(p) = pages.pop().filter(|p| *p != HOLE) {
                crate::mem::free_page(p);
            }
        }
        // the bytes of a partly used last page beyond the new end read as zero again
        if len % 4096 != 0 {
            if let Some(&last) = pages.last().filter(|p| **p != HOLE) {
                let from = (len % 4096) as usize;
                // SAFETY: `last` is a page of this object, identity-mapped in the kernel; the range lies in it.
                unsafe { core::ptr::write_bytes((last + from as u64) as *mut u8, 0, 4096 - from) };
            }
        }
        self.size.store(len, Ordering::SeqCst);
        Ok(())
    }

    /// `F_ADD_SEALS`.
    pub fn add_seals(&self, add: u32) -> Result<(), i64> {
        if add & !SEALS_ALL != 0 {
            return Err(-EINVAL);
        }
        if self.seals() & SEAL_SEAL != 0 {
            return Err(-EPERM);
        }
        // a write seal while a writable shared mapping exists would not hold
        if add & SEAL_WRITE != 0 && self.writable_maps.load(Ordering::SeqCst) > 0 {
            return Err(-EBUSY);
        }
        self.seals.fetch_or(add, Ordering::SeqCst);
        Ok(())
    }

    /// Copy out up to `n` bytes from `pos`.
    pub fn read(&self, pos: u64, n: usize) -> Vec<u8> {
        let size = self.size();
        let end = size.min(pos.saturating_add(n as u64));
        let mut out = Vec::new();
        if pos >= end || !crate::mem::try_reserve(&mut out, (end - pos) as usize) {
            return out;
        }
        let mut pages = self.pages.lock();
        if self.source.is_some() {
            // what a disk file holds is read now; a failed read leaves the rest as it reads: zero
            let (first, last) = ((pos / 4096) as usize, (end.div_ceil(4096) as usize).min(pages.len()));
            let _ = first < last && self.fill(&mut pages, first, last - first);
        }
        let mut at = pos;
        while at < end {
            let (page, off) = ((at / 4096) as usize, at % 4096);
            let k = (4096 - off).min(end - at);
            let Some(&pa) = pages.get(page) else { break };
            if pa == HOLE {
                out.resize(out.len() + k as usize, 0);
                at += k;
                continue;
            }
            // SAFETY: `pa` is a page of this object (identity-mapped); `off + k` stays inside it.
            out.extend_from_slice(unsafe { core::slice::from_raw_parts((pa + off) as *const u8, k as usize) });
            at += k;
        }
        out
    }

    /// Write `data` at `pos`, growing the object if needed (seals permitting).
    pub fn write(&self, pos: u64, data: &[u8]) -> Result<usize, i64> {
        if self.seals() & (SEAL_WRITE | SEAL_FUTURE_WRITE) != 0 {
            return Err(-EPERM);
        }
        let end = pos.checked_add(data.len() as u64).ok_or(-EFBIG)?;
        if end > self.size() {
            self.set_size(end)?;
        }
        let mut pages = self.pages.lock();
        let (first, last) = ((pos / 4096) as usize, (end.div_ceil(4096) as usize).min(pages.len()));
        if first < last && !self.fill(&mut pages, first, last - first) {
            return Err(-ENOMEM);
        }
        let mut done = 0usize;
        while done < data.len() {
            let at = pos + done as u64;
            let (page, off) = ((at / 4096) as usize, at % 4096);
            let k = ((4096 - off) as usize).min(data.len() - done);
            let Some(&pa) = pages.get(page) else { break };
            // SAFETY: `pa` is a page of this object (identity-mapped); `off + k` stays inside it.
            unsafe { core::ptr::copy_nonoverlapping(data[done..].as_ptr(), (pa + off) as *mut u8, k) };
            done += k;
        }
        Ok(done)
    }

    /// Physical addresses of the object's pages in `[off, off + len)` (both page-aligned); a range that
    /// reaches beyond the object yields only the pages it has. Holes in the range get their memory
    /// now: what is mapped is real. `None`: no memory for the pages or the list.
    pub fn frames(&self, off: u64, len: u64) -> Option<Vec<u64>> {
        let mut pages = self.pages.lock();
        let first = ((off / 4096) as usize).min(pages.len());
        let n = ((len / 4096) as usize).min(pages.len() - first);
        if !self.fill(&mut pages, first, n) {
            return None;
        }
        let slice = &pages[first..first + n];
        let mut out = Vec::new();
        if !crate::mem::try_reserve(&mut out, n) {
            return None;
        }
        out.extend_from_slice(slice);
        Some(out)
    }

    /// A mapping was made (`writable`: with write access) or removed.
    pub fn mapped(&self, writable: bool, add: bool) {
        let step = |c: &AtomicU64| {
            if add {
                c.fetch_add(1, Ordering::SeqCst);
            } else {
                c.fetch_sub(1, Ordering::SeqCst);
            }
        };
        step(&self.maps);
        if writable {
            step(&self.writable_maps);
        }
    }
}

impl Drop for MemObj {
    fn drop(&mut self) {
        for p in self.pages.get_mut().drain(..).filter(|p| *p != HOLE) {
            crate::mem::free_page(p);
        }
    }
}

/// What a shared mapping maps, from which byte offset: an object (kept alive by the mapping, counted in
/// its mappings) or the framebuffer.
pub enum Backing {
    Obj(Arc<MemObj>, bool, u64),
    Framebuffer(u64),
}

impl Backing {
    /// A new mapping of `obj` from `off` on.
    pub fn of(obj: Arc<MemObj>, writable: bool, off: u64) -> Backing {
        obj.mapped(writable, true);
        Backing::Obj(obj, writable, off)
    }

    /// Physical pages of the first `len` bytes of the mapping (fewer if the object or screen is smaller).
    pub fn frames(&self, len: u64) -> Option<Vec<u64>> {
        match self {
            Backing::Obj(o, _, off) => o.frames(*off, len),
            Backing::Framebuffer(off) => {
                let fb = crate::console::framebuffer()?;
                let size = (fb.stride as u64 * fb.height as u64 * 4).div_ceil(4096) * 4096;
                let n = len.min(size.saturating_sub(*off)) / 4096;
                let mut out = Vec::new();
                if !crate::mem::try_reserve(&mut out, n as usize) {
                    return None;
                }
                out.extend((0..n).map(|i| fb.base + off + i * 4096));
                Some(out)
            }
        }
    }

    /// Whether the mapping may be written (a sealed object refuses new writable mappings).
    pub fn writable(&self) -> bool {
        match self {
            Backing::Obj(_, w, _) => *w,
            Backing::Framebuffer(_) => true,
        }
    }
}

impl Clone for Backing {
    fn clone(&self) -> Backing {
        match self {
            Backing::Obj(o, w, off) => Backing::of(o.clone(), *w, *off),
            Backing::Framebuffer(off) => Backing::Framebuffer(*off),
        }
    }
}

impl Drop for Backing {
    fn drop(&mut self) {
        if let Backing::Obj(o, writable, _) = self {
            o.mapped(*writable, false);
        }
    }
}

/// `fallocate(fd, mode, off, len)` on a memory object (`None`: `fd` is none). Mode 0 grows the object
/// to `off + len` (seals permitting) and gives the range its memory; `FALLOC_FL_KEEP_SIZE` does the
/// same without growing; other modes are not supported.
pub fn fallocate(fd: u64, mode: u64, off: u64, len: u64) -> Option<i64> {
    let obj = crate::syscall::with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(crate::proc::FdKind::Mem(m, _)) => Some(m.clone()),
        _ => None,
    })
    .flatten()?;
    const KEEP_SIZE: u64 = 1;
    let Some(end) = off.checked_add(len).filter(|_| len > 0 && (off as i64) >= 0) else { return Some(-EINVAL) };
    if mode & !KEEP_SIZE != 0 {
        return Some(-95);
    }
    if mode == 0 && end > obj.size() {
        if let Err(e) = obj.set_size(end) {
            return Some(e);
        }
    }
    let first = off / 4096 * 4096;
    Some(if obj.frames(first, end.div_ceil(4096) * 4096 - first).is_some() { 0 } else { -ENOMEM })
}

/// `memfd_create(name, flags)`: the descriptor of a new object.
pub fn create(flags: u64) -> i64 {
    if flags & !(MFD_CLOEXEC | MFD_ALLOW_SEALING) != 0 {
        return -EINVAL;
    }
    let obj = Arc::new(MemObj::new(flags & MFD_ALLOW_SEALING != 0));
    let fd = crate::proc::Fd { kind: crate::proc::FdKind::Mem(obj, 0), cloexec: flags & MFD_CLOEXEC != 0, flags: 2 };
    crate::syscall::with(|p| p.add_fd(fd, 0)).unwrap_or(-9)
}
