//! Kernel heap (`alloc`) on top of the page allocator, with accounting per resource group.
//!
//! * Objects up to 2 KiB are blocks of fixed size classes in *slabs* (`slab.rs`): constant time.
//! * Objects below [`LARGE`] live in *chunks* (each a `linked_list_allocator` heap over a block of
//!   pages). Slabs and chunks are taken from the page allocator when needed and given back when
//!   empty, so the heap and the user pages share one pool of memory.
//! * Bigger objects are page runs straight from the page allocator.
//! * Every object is charged to the group of the current *context* (the group of the running
//!   process, or the one set with [`scope`]); a 16-byte header in front of a small object and the
//!   page record of a run remember the owner, so freeing un-charges the right group no matter who
//!   frees.
//!
//! Rust's collections cannot report a failed allocation, so a request is refused by *policy*
//! (limits, fair share, reserve) only inside [`fallible`] and only from one page up – the code
//! there uses `try_reserve` and turns the refusal into `ENOMEM`. Everything else is *forced*: charged without a limit check and
//! served as long as memory physically exists, from the kernel reserve and finally from a separate
//! emergency heap. The limits catch up at the next safe point (end of the system call, see
//! `proc::enforce`). Only when even the emergency heap is exhausted does an allocation fail.

use crate::groups::{self, Denied};
use crate::mem::PAGE;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU64, Ordering};
use linked_list_allocator::Heap;

/// Requests of at least this size (with header) are page runs.
pub const LARGE: usize = 64 * 1024;
const HEADER: usize = 16;
const MAX_CHUNKS: usize = 2048;
const MAGIC: u32 = 0x554c_5441;

struct Small {
    heaps: [Heap; MAX_CHUNKS],
    /// Start address of the chunk in each slot; 0 = slot unused.
    base: [u64; MAX_CHUNKS],
    /// Pages of the chunk in each slot.
    pages: [u32; MAX_CHUNKS],
    /// Slots below this index may be in use.
    high: usize,
    /// Number of chunks that hold no object.
    empty: usize,
    held: u64,
    used: u64,
}

static SMALL: spin::Mutex<Small> = spin::Mutex::new(Small {
    heaps: [const { Heap::empty() }; MAX_CHUNKS],
    base: [0; MAX_CHUNKS],
    pages: [0; MAX_CHUNKS],
    high: 0,
    empty: 0,
    held: 0,
    used: 0,
});
static EMERGENCY: spin::Mutex<Heap> = spin::Mutex::new(Heap::empty());
static EMERGENCY_RANGE: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static EMERGENCY_USES: AtomicU64 = AtomicU64::new(0);
static CHUNK_PAGES: AtomicU64 = AtomicU64::new(512);
/// Smallest chunk (pages) the heap falls back to when memory is too fragmented for a full one;
/// it must hold the largest small object.
const MIN_CHUNK_PAGES: u64 = 2 * (LARGE as u64) / PAGE;

struct KernelAlloc;

#[global_allocator]
static ALLOC: KernelAlloc = KernelAlloc;

/// Set up the emergency heap (called once from `mem::init`).
///
/// # Safety
/// `[start, start + size)` must be unused, identity-mapped memory that is never handed out otherwise.
// SAFETY: (contract) the caller hands over a range nobody else uses – see `# Safety` above.
pub unsafe fn init_emergency(start: u64, size: u64, total: u64) {
    EMERGENCY.lock().init(start as *mut u8, size as usize);
    EMERGENCY_RANGE[0].store(start, Ordering::Relaxed);
    EMERGENCY_RANGE[1].store(start + size, Ordering::Relaxed);
    // chunk size grows with the machine: 2 MiB up to 4 GiB of memory, at most 16 MiB
    let mut pages = 512u64;
    while pages < 4096 && total > (pages / 512) * (4 << 30) {
        pages *= 2;
    }
    CHUNK_PAGES.store(pages, Ordering::Relaxed);
}

// ------------------------------------------------------------------ context

/// Group that allocations on this processor are charged to.
pub fn context() -> u16 {
    crate::percpu::me().charge as u16
}

/// Set the charged group (the scheduler does this when a process starts to run).
pub fn set_context(group: u16) {
    crate::percpu::me().charge = group as u64;
}

/// Run `f` with allocations charged to `group`.
pub fn scope<R>(group: u16, f: impl FnOnce() -> R) -> R {
    let old = core::mem::replace(&mut crate::percpu::me().charge, group as u64);
    let r = f();
    crate::percpu::me().charge = old;
    r
}

/// Run `f` with allocations charged to the kernel itself (shared tables that outlive a process).
pub fn kernel<R>(f: impl FnOnce() -> R) -> R {
    scope(groups::ROOT, f)
}

/// Run `f` – code that handles a failed allocation (`try_reserve`) – with policy applied: a request
/// that the limits refuse fails instead of being forced. A refusal is remembered for the system
/// call in progress ([`take_refused`]).
pub fn fallible<R, E>(f: impl FnOnce() -> Result<R, E>) -> Result<R, E> {
    let cpu = crate::percpu::me();
    cpu.fallible += 1;
    let r = f();
    let cpu = crate::percpu::me();
    cpu.fallible = cpu.fallible.saturating_sub(1);
    r
}

/// Remember why a request was refused.
pub fn note_denial(d: Denied) {
    crate::percpu::me().denied = d as u64 + 1;
}

/// Reason of the last refused request on this processor since [`clear_refused`].
pub fn last_denial() -> Option<Denied> {
    Denied::from_code(crate::percpu::me().denied)
}

/// Forget refusals (start of a system call or fault).
pub fn clear_refused() {
    let cpu = crate::percpu::me();
    cpu.denied = 0;
    cpu.fallible = 0;
}

/// Whether a request was refused since [`clear_refused`]; clears the mark.
pub fn take_refused() -> bool {
    core::mem::replace(&mut crate::percpu::me().denied, 0) != 0
}

// ------------------------------------------------------------------ small objects

impl Small {
    fn alloc(&mut self, layout: Layout, keep_reserve: bool) -> Option<NonNull<u8>> {
        for i in 0..self.high {
            if self.base[i] != 0 && self.heaps[i].free() >= layout.size() {
                let was_empty = self.heaps[i].used() == 0;
                if let Ok(p) = self.heaps[i].allocate_first_fit(layout) {
                    if was_empty {
                        self.empty = self.empty.saturating_sub(1);
                    }
                    self.used += layout.size() as u64;
                    return Some(p);
                }
            }
        }
        let slot = (0..MAX_CHUNKS).find(|&i| self.base[i] == 0)?;
        // a full chunk, or – when no block of that size is left – the largest smaller one
        let mut pages = CHUNK_PAGES.load(Ordering::Relaxed);
        let addr = loop {
            match crate::mem::alloc_chunk(pages, slot as u16, keep_reserve) {
                Some(a) => break a,
                None if pages > MIN_CHUNK_PAGES => pages /= 2,
                None => return None,
            }
        };
        // SAFETY: the chunk was just taken from the page allocator and belongs to this slot alone.
        unsafe { self.heaps[slot].init(addr as *mut u8, (pages * PAGE) as usize) };
        self.base[slot] = addr;
        self.pages[slot] = pages as u32;
        self.high = self.high.max(slot + 1);
        self.held += pages * PAGE;
        let p = self.heaps[slot].allocate_first_fit(layout).ok();
        match p {
            Some(_) => self.used += layout.size() as u64,
            None => self.empty += 1,
        }
        p
    }

    /// # Safety
    /// `ptr` must come from [`Small::alloc`] with the same layout and lie in chunk `slot`.
    // SAFETY: (contract) `ptr`/`layout` name a live object of chunk `slot` – see `# Safety` above.
    unsafe fn free(&mut self, slot: usize, ptr: NonNull<u8>, layout: Layout) {
        if slot >= MAX_CHUNKS || self.base[slot] == 0 {
            return;
        }
        self.heaps[slot].deallocate(ptr, layout);
        self.used = self.used.saturating_sub(layout.size() as u64);
        if self.heaps[slot].used() != 0 {
            return;
        }
        // keep one empty chunk so that a single object allocated and freed in a loop does not
        // take and return a chunk each time
        if self.empty == 0 {
            self.empty = 1;
            return;
        }
        let pages = self.pages[slot] as u64;
        let addr = core::mem::replace(&mut self.base[slot], 0);
        self.heaps[slot] = Heap::empty();
        self.held -= pages * PAGE;
        crate::mem::free_chunk(addr, pages);
    }
}

/// (bytes held in chunks, bytes used by objects)
pub fn small_stats() -> (u64, u64) {
    let s = SMALL.lock();
    (s.held, s.used)
}

/// How often the emergency heap had to serve a request. Stays 0 unless the kernel reserve was used up.
pub fn emergency_uses() -> u64 {
    EMERGENCY_USES.load(Ordering::Relaxed)
}

#[repr(C)]
struct Header {
    owner: u16,
    _pad: u16,
    magic: u32,
    charged: u64,
}

/// Layout actually requested from a chunk for the user layout: room for the header in front,
/// keeping the alignment. Returns (inner layout, offset of the user pointer).
fn with_header(layout: Layout) -> Option<(Layout, usize)> {
    let align = layout.align().max(HEADER);
    let inner = Layout::from_size_align(layout.size().checked_add(align)?, align).ok()?;
    Some((inner, align))
}

fn in_emergency(p: u64) -> bool {
    p >= EMERGENCY_RANGE[0].load(Ordering::Relaxed)
        && p < EMERGENCY_RANGE[1].load(Ordering::Relaxed)
}

/// # Safety
/// `base` must point to at least `off` writable bytes, `off >= HEADER`.
// SAFETY: (contract) the block at `base` is at least `off >= HEADER` bytes long – see `# Safety` above.
unsafe fn finish(base: NonNull<u8>, off: usize, owner: u16, charged: u64) -> *mut u8 {
    let user = base.as_ptr().add(off);
    (user.sub(HEADER) as *mut Header).write(Header {
        owner,
        _pad: 0,
        magic: MAGIC,
        charged,
    });
    user
}

/// Size class of the slab block for the layout, if it is a slab object.
fn slab_class(layout: Layout) -> Option<usize> {
    if layout.align() > HEADER || layout.size() + HEADER > crate::slab::MAX_BLOCK {
        return None;
    }
    crate::slab::class_for(layout.size() + HEADER)
}

fn alloc_small(layout: Layout, owner: u16, policy: bool) -> *mut u8 {
    let Some((inner, off)) = with_header(layout) else {
        return core::ptr::null_mut();
    };
    let class = slab_class(layout);
    let bytes = class.map_or(inner.size(), crate::slab::block_size) as u64;
    if policy {
        if let Err(d) = groups::try_charge(owner, bytes) {
            note_denial(d);
            return core::ptr::null_mut();
        }
    } else {
        groups::charge_forced(owner, bytes);
    }
    let p = match class {
        Some(c) => crate::slab::alloc(c, policy),
        None => SMALL.lock().alloc(inner, policy),
    };
    let p = match p {
        Some(p) => Some(p),
        None if policy => None,
        None => emergency(inner),
    };
    match p {
        // SAFETY: the block has `inner.size()` bytes and `off >= HEADER`.
        Some(p) => unsafe { finish(p, off, owner, bytes) },
        None => {
            groups::uncharge(owner, bytes);
            if policy {
                note_denial(Denied::Reserve);
            }
            core::ptr::null_mut()
        }
    }
}

fn emergency(inner: Layout) -> Option<NonNull<u8>> {
    let p = EMERGENCY.lock().allocate_first_fit(inner).ok()?;
    EMERGENCY_USES.fetch_add(1, Ordering::Relaxed);
    groups::memory_critical();
    Some(p)
}

/// Pages of a run for the layout (alignment above a page is met by a block of that size).
fn run_pages(layout: Layout) -> u64 {
    let pages = (layout.size() as u64).div_ceil(PAGE).max(1);
    if layout.align() as u64 > PAGE {
        pages.next_power_of_two().max(layout.align() as u64 / PAGE)
    } else {
        pages
    }
}

fn alloc_large(layout: Layout, owner: u16, policy: bool) -> *mut u8 {
    match crate::mem::alloc_run(run_pages(layout), owner, policy) {
        Ok(a) => a as *mut u8,
        Err(d) if policy => {
            note_denial(d);
            core::ptr::null_mut()
        }
        Err(_) => {
            // forced and no run of that size left: last resort is the emergency heap
            let Some((inner, off)) = with_header(layout) else {
                return core::ptr::null_mut();
            };
            let bytes = inner.size() as u64;
            match emergency(inner) {
                Some(p) => {
                    groups::charge_forced(owner, bytes);
                    // SAFETY: as in `alloc_small`.
                    unsafe { finish(p, off, owner, bytes) }
                }
                None => core::ptr::null_mut(),
            }
        }
    }
}

fn is_small(layout: Layout) -> bool {
    layout.size() + layout.align().max(HEADER) < LARGE && layout.align() <= PAGE as usize
}

// SAFETY: `alloc` returns blocks of at least `layout.size()` bytes with the requested alignment that
// overlap no other live block (slab block, chunk object or page run), or null; `dealloc` gives exactly
// such a block back to the allocator it came from, found from the address and the layout.
unsafe impl GlobalAlloc for KernelAlloc {
    // SAFETY: (contract of `GlobalAlloc`) the layout has a non-zero size; nothing else is assumed.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let cpu = crate::percpu::me();
        // requests below a page are always forced: the many tiny buffers of a system call must not
        // fail just because the caller's group is at its limit or memory is short (the reserve is
        // there for them); what counts – pages and buffers – is at least a page
        let (owner, policy) = (
            cpu.charge as u16,
            cpu.fallible != 0 && layout.size() >= PAGE as usize,
        );
        if is_small(layout) {
            alloc_small(layout, owner, policy)
        } else {
            alloc_large(layout, owner, policy)
        }
    }

    // SAFETY: (contract of `GlobalAlloc`) `ptr` came from `alloc` with this layout and is freed once.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let addr = ptr as u64;
        let headed = in_emergency(addr) || is_small(layout);
        if !headed {
            crate::mem::free_run(addr, run_pages(layout));
            return;
        }
        let Some((inner, off)) = with_header(layout) else {
            return;
        };
        // SAFETY: small and emergency objects carry a header right in front of the user pointer.
        let h = (ptr.sub(HEADER) as *const Header).read();
        if h.magic != MAGIC {
            return;
        }
        (ptr.sub(HEADER) as *mut Header).write(Header {
            owner: 0,
            _pad: 0,
            magic: 0,
            charged: 0,
        });
        // SAFETY: `off` bytes in front of the user pointer belong to the same block.
        let base = NonNull::new_unchecked(ptr.sub(off));
        if in_emergency(addr) {
            EMERGENCY.lock().deallocate(base, inner);
        } else {
            match crate::mem::heap_page(addr) {
                Some(crate::mem::HeapPage::Slab(class)) => crate::slab::free(base, class as usize),
                Some(crate::mem::HeapPage::Chunk(slot)) => {
                    SMALL.lock().free(slot as usize, base, inner)
                }
                None => {}
            }
        }
        groups::uncharge(h.owner, h.charged);
    }
}
