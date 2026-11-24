//! The buddy allocator behind `mem.rs`: free blocks of 2^0 … 2^16 pages per zone, merged on free, a
//! record per page, and a cache of single pages in front. Pure bookkeeping – charging, the reserve
//! policy and zeroing are done by `mem.rs`, which holds the one instance behind a lock.

use crate::mem::PAGE;
use core::sync::atomic::{AtomicU64, Ordering};

/// Free pages (including cached ones), readable without the lock.
pub(crate) static FREE_PAGES: AtomicU64 = AtomicU64::new(0);

/// Largest block of the buddy allocator: 2^16 pages = 256 MiB.
pub(crate) const MAX_ORDER: usize = 16;
/// Addresses below this line can be reached by every device (32-bit DMA).
const DMA_LIMIT: u64 = 1 << 32;

const ST_NONE: u8 = 0;
const ST_FREE: u8 = 1;
pub(crate) const ST_USED: u8 = 2;
pub(crate) const ST_HEAP: u8 = 3;
pub(crate) const ST_SLAB: u8 = 4;

/// Per-page record. `state`/`order` describe a free block at its first page; `owner` is the group
/// charged for an allocated block (first page), the heap chunk slot (`ST_HEAP`, every page) or the
/// size class of a slab (`ST_SLAB`, every page).
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Meta {
    pub(crate) state: u8,
    pub(crate) order: u8,
    pub(crate) owner: u16,
}

/// Single pages kept ready in front of the buddy lists (taking and returning one page is the
/// common case and needs no splitting or merging this way).
pub(crate) const CACHE: usize = 512;
/// An empty cache is refilled with one block of 2^5 = 32 pages.
const REFILL_ORDER: usize = 5;

pub(crate) struct Buddy {
    /// List heads per zone (0: below 4 GiB, 1: above) and order; 0 = empty.
    pub(crate) head: [[u64; MAX_ORDER + 1]; 2],
    pub(crate) meta: *mut Meta,
    pub(crate) base: u64,
    pub(crate) pages: u64,
    /// Free pages, including the cached ones.
    pub(crate) free: u64,
    pub(crate) cache: [u64; CACHE],
    pub(crate) cached: usize,
}

// SAFETY: the raw pointer refers to the page records, which are only touched under the lock.
unsafe impl Send for Buddy {}

fn zone(addr: u64) -> usize {
    (addr >= DMA_LIMIT) as usize
}

fn order_for(pages: u64) -> usize {
    pages.next_power_of_two().trailing_zeros() as usize
}

impl Buddy {
    pub(crate) fn meta(&mut self, addr: u64) -> Option<&mut Meta> {
        if addr < self.base || (addr - self.base) / PAGE >= self.pages {
            return None;
        }
        // SAFETY: the index was checked against the size of the record array set up in `init`.
        Some(unsafe { &mut *self.meta.add(((addr - self.base) / PAGE) as usize) })
    }

    fn push(&mut self, addr: u64, order: usize) {
        let z = zone(addr);
        let next = self.head[z][order];
        // SAFETY: `addr` is a free, identity-mapped page owned by the allocator; its first 16 bytes
        // hold the list links while it is free.
        unsafe {
            (addr as *mut [u64; 2]).write([next, 0]);
            if next != 0 {
                (*(next as *mut [u64; 2]))[1] = addr;
            }
        }
        self.head[z][order] = addr;
        if let Some(m) = self.meta(addr) {
            m.state = ST_FREE;
            m.order = order as u8;
        }
    }

    fn unlink(&mut self, addr: u64, order: usize) {
        // SAFETY: `addr` is the first page of a block on the free list (state `ST_FREE`).
        let [next, prev] = unsafe { (addr as *const [u64; 2]).read() };
        // SAFETY: the neighbours are list members, i.e. free pages owned by the allocator.
        unsafe {
            if prev != 0 {
                (*(prev as *mut [u64; 2]))[0] = next;
            } else {
                self.head[zone(addr)][order] = next;
            }
            if next != 0 {
                (*(next as *mut [u64; 2]))[1] = prev;
            }
        }
        if let Some(m) = self.meta(addr) {
            m.state = ST_NONE;
        }
    }

    /// Free one block and merge it with its buddy as far as possible.
    fn free_block(&mut self, mut addr: u64, mut order: usize) {
        self.free += 1 << order;
        while order < MAX_ORDER {
            let buddy = addr ^ (PAGE << order);
            let mergeable = zone(buddy) == zone(addr)
                && matches!(self.meta(buddy), Some(m) if m.state == ST_FREE && m.order as usize == order);
            if !mergeable {
                break;
            }
            self.unlink(buddy, order);
            addr = addr.min(buddy);
            order += 1;
        }
        self.push(addr, order);
    }

    /// Free the pages `[start, end)` as the largest aligned blocks.
    pub(crate) fn free_range(&mut self, mut start: u64, end: u64) {
        while start < end {
            let by_align = (start / PAGE).trailing_zeros() as usize;
            let by_len = 63 - ((end - start) / PAGE).leading_zeros() as usize;
            let order = by_align.min(by_len).min(MAX_ORDER);
            self.free_block(start, order);
            start += PAGE << order;
        }
    }

    fn take_block(&mut self, order: usize, low_first: bool) -> Option<u64> {
        let zones = if low_first { [0, 1] } else { [1, 0] };
        for z in zones {
            for k in order..=MAX_ORDER {
                let addr = self.head[z][k];
                if addr == 0 {
                    continue;
                }
                self.unlink(addr, k);
                let mut k = k;
                while k > order {
                    k -= 1;
                    self.push(addr + (PAGE << k), k);
                }
                self.free -= 1 << order;
                return Some(addr);
            }
        }
        None
    }

    /// Put the cached pages back into the buddy lists (so that they can merge into larger blocks).
    fn flush_cache(&mut self) {
        while self.cached > 0 {
            self.cached -= 1;
            let addr = self.cache[self.cached];
            // `free_block` counts the page as freed; it was counted while it sat in the cache
            self.free -= 1;
            self.free_block(addr, 0);
        }
    }

    /// `pages` contiguous pages; fails if fewer than `keep` pages would stay free.
    pub(crate) fn take(
        &mut self,
        pages: u64,
        keep: u64,
        low_first: bool,
        state: u8,
        owner: u16,
    ) -> Option<u64> {
        if pages == 0 || self.free < pages + keep {
            return None;
        }
        // one page for a process: from the cache; an empty cache is refilled with a whole block
        if pages == 1 && state == ST_USED && !low_first {
            let addr = if self.cached > 0 {
                self.cached -= 1;
                self.free -= 1;
                Some(self.cache[self.cached])
            } else if let Some(block) = self.take_block(REFILL_ORDER, false) {
                // `take_block` took all of them off the count; all but the first stay free (cached)
                for i in 1..1u64 << REFILL_ORDER {
                    self.cache[self.cached] = block + i * PAGE;
                    self.cached += 1;
                }
                self.free += (1 << REFILL_ORDER) - 1;
                Some(block)
            } else {
                None
            };
            if let Some(addr) = addr {
                if let Some(m) = self.meta(addr) {
                    *m = Meta {
                        state,
                        order: 0,
                        owner,
                    };
                }
                FREE_PAGES.store(self.free, Ordering::Relaxed);
                return Some(addr);
            }
        }
        let order = order_for(pages);
        if order > MAX_ORDER {
            return None;
        }
        let addr = match self.take_block(order, low_first) {
            Some(a) => a,
            None => {
                // the cached pages may be what a block of this size is missing
                self.flush_cache();
                self.take_block(order, low_first)?
            }
        };
        self.free_range(addr + pages * PAGE, addr + (PAGE << order));
        let all = state != ST_USED;
        for i in 0..if all { pages } else { 1 } {
            if let Some(m) = self.meta(addr + i * PAGE) {
                *m = Meta {
                    state,
                    order: 0,
                    owner,
                };
            }
        }
        FREE_PAGES.store(self.free, Ordering::Relaxed);
        Some(addr)
    }

    /// Give `pages` pages at `addr` back; returns the recorded owner, `None` for a block that is not
    /// allocated (double free – ignored and counted).
    pub(crate) fn give(&mut self, addr: u64, pages: u64, state: u8) -> Option<u16> {
        let owner = match self.meta(addr) {
            Some(m) if m.state == state => m.owner,
            _ => return None,
        };
        let all = state != ST_USED;
        for i in 0..if all { pages } else { 1 } {
            if let Some(m) = self.meta(addr + i * PAGE) {
                m.state = ST_NONE;
            }
        }
        if pages == 1 && state == ST_USED && self.cached < CACHE {
            self.cache[self.cached] = addr;
            self.cached += 1;
            self.free += 1;
        } else {
            self.free_range(addr, addr + pages * PAGE);
        }
        FREE_PAGES.store(self.free, Ordering::Relaxed);
        Some(owner)
    }
}
