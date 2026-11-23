//! Physical memory of the Ultar kernel: page frames from the UEFI memory map, handed out by a buddy
//! allocator. Everything else builds on it: user pages and page tables (`paging.rs`), DMA buffers of
//! the drivers, and the kernel heap (`heap.rs`).
//!
//! The firmware mapped all memory 1:1 (virtual = physical) and the kernel keeps that, so a page
//! address is also the pointer to it.
//!
//! Every page that is handed out is charged to a resource group (`groups.rs`, plan §3.7): the page
//! header remembers the owner, freeing un-charges it. Requests come in two kinds:
//!
//! * **with policy** ([`alloc_page`], fallible heap requests): limits of the group, the protected
//!   minimum of `system`/`session`, the fair-share rule and the kernel reserve apply; a refusal is
//!   reported to the caller (`ENOMEM`, or the end of the faulting process) – never a panic;
//! * **forced** (small kernel objects, DMA): only physical exhaustion can refuse them, and the
//!   reserve below [`reserve_bytes`] exists so that this does not happen while the kernel handles
//!   a shortage.

use crate::buddy::{Buddy, Meta, CACHE, FREE_PAGES, MAX_ORDER, ST_HEAP, ST_SLAB, ST_USED};
use crate::efi::Boot;
use crate::groups::{self, Denied};
use core::sync::atomic::{AtomicU64, Ordering};

/// Size of a page frame.
pub const PAGE: u64 = 4096;
static BUDDY: spin::Mutex<Buddy> = spin::Mutex::new(Buddy { head: [[0; MAX_ORDER + 1]; 2], meta: core::ptr::null_mut(), base: 0, pages: 0, free: 0, cache: [0; CACHE], cached: 0 });
static TOTAL: AtomicU64 = AtomicU64::new(0);
static REGIONS: AtomicU64 = AtomicU64::new(0);
static RESERVE: AtomicU64 = AtomicU64::new(0);
static BAD_FREE: AtomicU64 = AtomicU64::new(0);
/// Page records for readers without the lock: (address of the array, first address covered, pages).
static META: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

/// Highest address of the working memory according to the firmware map.
pub fn top(boot: &Boot) -> u64 {
    boot.regions().filter(|d| matches!(d.typ, 1..=7 | 9 | 10)).map(|d| d.phys + d.pages * PAGE).max().unwrap_or(1 << 32)
}

const MAXR: usize = 256;

/// Usable regions of the firmware map as `(start, end)`, without the firmware stack we still run on
/// and without the window for fixed-address programs (see paging.rs); nothing below 1 MiB.
// SAFETY: (contract) only reads the stack pointer and the firmware map; called once from `init`.
unsafe fn usable_regions(boot: &Boot, out: &mut [(u64, u64); MAXR]) -> usize {
    let rsp: u64;
    core::arch::asm!("mov {}, rsp", out(reg) rsp);
    let (s0, s1) = ((rsp & !0x1f_ffff).saturating_sub(0x20_0000), (rsp | 0x1f_ffff) + 1);
    let low = crate::paging::USER_LOW_END;
    let mut n = 0;
    let mut add = |start: u64, end: u64| {
        let (start, end) = ((start + PAGE - 1) & !(PAGE - 1), end & !(PAGE - 1));
        if end > start + PAGE && n < MAXR {
            out[n] = (start, end);
            n += 1;
        }
    };
    for d in boot.regions() {
        if !crate::efi::usable(d.typ) {
            continue;
        }
        let start = d.phys.max(low);
        let end = d.phys + d.pages * PAGE;
        if end <= start {
            continue;
        }
        if start < s1 && s0 < end {
            add(start, s0.max(start));
            add(s1.min(end), end);
        } else {
            add(start, end);
        }
    }
    n
}

/// Take over the memory map: page records, the emergency heap, then every free page.
///
/// # Safety
/// Must run once, on the boot processor, after the kernel's own page tables are active.
// SAFETY: (contract) runs once on the boot processor with the kernel's page tables – see `# Safety` above.
pub unsafe fn init(boot: &Boot) {
    let mut regions = [(0u64, 0u64); MAXR];
    let n = usable_regions(boot, &mut regions);
    let regions = &mut regions[..n];
    let (Some(lo), Some(hi)) = (regions.iter().map(|r| r.0).min(), regions.iter().map(|r| r.1).max()) else { return };
    let pages = (hi - lo) / PAGE;
    let meta_bytes = (pages * core::mem::size_of::<Meta>() as u64 + PAGE - 1) & !(PAGE - 1);
    let sum: u64 = regions.iter().map(|r| r.1 - r.0).sum();
    let emergency = (sum / 256).clamp(1 << 20, 8 << 20) & !(PAGE - 1);
    // records and emergency heap come out of the largest region
    let Some(best) = regions.iter_mut().max_by_key(|r| r.1 - r.0) else { return };
    if best.1 - best.0 < meta_bytes + emergency + (4 << 20) {
        return;
    }
    let meta = best.0 as *mut Meta;
    core::ptr::write_bytes(meta as *mut u8, 0, meta_bytes as usize);
    crate::heap::init_emergency(best.0 + meta_bytes, emergency, sum);
    best.0 += meta_bytes + emergency;
    let mut b = BUDDY.lock();
    b.meta = meta;
    b.base = lo;
    b.pages = pages;
    META[0].store(meta as u64, Ordering::Relaxed);
    META[1].store(lo, Ordering::Relaxed);
    META[2].store(pages, Ordering::Relaxed);
    for r in regions.iter() {
        b.free_range(r.0, r.1);
    }
    let total = b.free * PAGE;
    FREE_PAGES.store(b.free, Ordering::Relaxed);
    TOTAL.store(total, Ordering::Relaxed);
    REGIONS.store(n as u64, Ordering::Relaxed);
    RESERVE.store((total / 64).clamp(4 << 20, 64 << 20).min(total / 4), Ordering::Relaxed);
    drop(b);
    groups::init(total);
}

/// Bytes the kernel keeps for itself: requests with policy are refused before free memory falls
/// below this line; only forced requests (small kernel objects) may go lower.
pub fn reserve_bytes() -> u64 {
    RESERVE.load(Ordering::Relaxed)
}

fn reserve_pages() -> u64 {
    reserve_bytes() / PAGE
}

/// All memory managed by the page allocator, in bytes.
pub fn total_bytes() -> u64 {
    TOTAL.load(Ordering::Relaxed)
}

/// Free memory in bytes.
pub fn free_bytes() -> u64 {
    FREE_PAGES.load(Ordering::Relaxed) * PAGE
}

/// `n` contiguous zeroed pages for a driver (DMA: physical address = pointer), preferably below
/// 4 GiB. Forced, charged to the kernel, never given back.
pub fn alloc_pages(n: usize) -> Option<u64> {
    let a = BUDDY.lock().take(n as u64, 0, true, ST_USED, groups::ROOT)?;
    groups::charge_forced(groups::ROOT, n as u64 * PAGE);
    // SAFETY: the pages were just taken from the allocator and are identity-mapped.
    unsafe { core::ptr::write_bytes(a as *mut u8, 0, n * PAGE as usize) };
    Some(a)
}

/// One zeroed page for a process (user page or page table), charged to the current group
/// ([`crate::heap::context`]). Policy applies: `None` means refused, the reason is in
/// [`crate::heap::last_denial`].
pub fn alloc_page() -> Option<u64> {
    let g = crate::heap::context();
    if let Err(d) = groups::try_charge(g, PAGE) {
        crate::heap::note_denial(d);
        return None;
    }
    let Some(a) = BUDDY.lock().take(1, reserve_pages(), false, ST_USED, g) else {
        groups::uncharge(g, PAGE);
        crate::heap::note_denial(Denied::Reserve);
        return None;
    };
    // SAFETY: as in `alloc_pages`.
    unsafe { core::ptr::write_bytes(a as *mut u8, 0, PAGE as usize) };
    Some(a)
}

/// One zeroed page for the kernel's own page tables: forced.
pub fn alloc_page_kernel() -> Option<u64> {
    alloc_pages(1)
}

/// Give a page from [`alloc_page`] back.
pub fn free_page(p: u64) {
    free_run(p, 1);
}

/// `pages` contiguous pages for the kernel heap (not zeroed), charged to `owner`.
/// `policy`: apply limits and keep the reserve; otherwise forced.
pub(crate) fn alloc_run(pages: u64, owner: u16, policy: bool) -> Result<u64, Denied> {
    let bytes = pages * PAGE;
    if policy {
        groups::try_charge(owner, bytes)?;
    } else {
        groups::charge_forced(owner, bytes);
    }
    let keep = if policy { reserve_pages() } else { 0 };
    match BUDDY.lock().take(pages, keep, false, ST_USED, owner) {
        Some(a) => Ok(a),
        None => {
            groups::uncharge(owner, bytes);
            Err(Denied::Reserve)
        }
    }
}

/// Give a run from [`alloc_run`] (or a single page) back and un-charge its owner.
pub(crate) fn free_run(addr: u64, pages: u64) {
    let owner = BUDDY.lock().give(addr, pages, ST_USED);
    match owner {
        Some(g) => groups::uncharge(g, pages * PAGE),
        None => {
            BAD_FREE.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// A chunk for the small-object heap: `pages` pages marked with the chunk slot. Not charged (the
/// objects inside are). `keep_reserve`: do not go below the kernel reserve.
pub(crate) fn alloc_chunk(pages: u64, slot: u16, keep_reserve: bool) -> Option<u64> {
    BUDDY.lock().take(pages, if keep_reserve { reserve_pages() } else { 0 }, false, ST_HEAP, slot)
}

/// Give a heap chunk back.
pub(crate) fn free_chunk(addr: u64, pages: u64) {
    if BUDDY.lock().give(addr, pages, ST_HEAP).is_none() {
        BAD_FREE.fetch_add(1, Ordering::Relaxed);
    }
}

/// Pages for a slab of small objects, every page marked with the size class. Not charged (the
/// objects inside are).
pub(crate) fn alloc_slab(pages: u64, class: u16, keep_reserve: bool) -> Option<u64> {
    BUDDY.lock().take(pages, if keep_reserve { reserve_pages() } else { 0 }, false, ST_SLAB, class)
}

/// Give a slab back.
pub(crate) fn free_slab(addr: u64, pages: u64) {
    if BUDDY.lock().give(addr, pages, ST_SLAB).is_none() {
        BAD_FREE.fetch_add(1, Ordering::Relaxed);
    }
}

/// What the kernel heap keeps at an address.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum HeapPage {
    /// A chunk; the slot number.
    Chunk(u16),
    /// A slab; the size class.
    Slab(u16),
}

/// Whether `addr` lies in a heap chunk or a slab. Read without the allocator's lock: the record of
/// a page cannot change while an object in it is alive, and the caller holds such an object.
pub(crate) fn heap_page(addr: u64) -> Option<HeapPage> {
    let (meta, base, pages) = (META[0].load(Ordering::Relaxed) as *const Meta, META[1].load(Ordering::Relaxed), META[2].load(Ordering::Relaxed));
    if meta.is_null() || addr < base || (addr - base) / PAGE >= pages {
        return None;
    }
    // SAFETY: the index was checked against the array's size; see above for the race.
    let m = unsafe { meta.add(((addr - base) / PAGE) as usize).read() };
    match m.state {
        ST_HEAP => Some(HeapPage::Chunk(m.owner)),
        ST_SLAB => Some(HeapPage::Slab(m.owner)),
        _ => None,
    }
}

/// (total bytes, bytes held by the small-object heap, bytes used in it, regions of the memory map)
pub fn stats() -> (u64, u64, u64, u64) {
    let (chunks, slabs) = (crate::heap::small_stats(), crate::slab::stats());
    let (held, used) = (chunks.0 + slabs.0, chunks.1 + slabs.1);
    (total_bytes(), held, used, REGIONS.load(Ordering::Relaxed))
}

/// Number of frees the page allocator refused (a block that was not allocated). Must stay 0.
pub fn bad_frees() -> u64 {
    BAD_FREE.load(Ordering::Relaxed)
}
pub use crate::trybuf::{try_copy, try_reserve, try_vec, try_with_capacity};
