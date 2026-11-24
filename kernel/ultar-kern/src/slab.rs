//! Small kernel objects: blocks of fixed size classes cut from *slabs* (one or four pages from the
//! page allocator). Taking and giving back a block is a constant number of steps, however many
//! objects exist; a slab without objects goes back to the page allocator.
//!
//! A slab starts with a [`Head`]; the rest is blocks of one class. Free blocks of a slab form a
//! list through their first word. Slabs that have a free block are linked per class. The page
//! records (`mem.rs`) carry the class for every page of a slab, so a block's slab is found from its
//! address alone.
//!
//! Used by `heap.rs` for requests up to [`MAX_BLOCK`] bytes with an alignment of at most 16.

use crate::mem::PAGE;
use core::ptr::NonNull;

/// Block sizes in bytes (each a multiple of 16).
const CLASSES: [u32; 13] = [32, 48, 64, 96, 128, 192, 256, 384, 512, 768, 1024, 1536, 2048];
/// Largest block.
pub const MAX_BLOCK: usize = 2048;
const HEAD: u64 = 32;

/// Start of every slab. `free`: first free block (0 = none); `used`: blocks handed out;
/// `next`/`prev`: neighbours in the class's list of slabs with a free block (0 = none).
#[repr(C)]
struct Head {
    free: u64,
    used: u32,
    class: u32,
    next: u64,
    prev: u64,
}

struct Slabs {
    /// Per class: first slab with a free block (0 = none).
    partial: [u64; CLASSES.len()],
    /// Bytes held in slabs / bytes handed out as blocks.
    held: u64,
    used: u64,
}

static SLABS: spin::Mutex<Slabs> = spin::Mutex::new(Slabs { partial: [0; CLASSES.len()], held: 0, used: 0 });

/// Class for a block of `size` bytes, if it is small enough.
pub fn class_for(size: usize) -> Option<usize> {
    CLASSES.iter().position(|&c| c as usize >= size)
}

/// Bytes of a block of class `class`.
pub fn block_size(class: usize) -> usize {
    CLASSES[class] as usize
}

/// Pages of one slab of the class: one page up to 512-byte blocks, four above.
fn slab_pages(class: usize) -> u64 {
    if CLASSES[class] <= 512 {
        1
    } else {
        4
    }
}

/// # Safety
/// `slab` must be the start address of a live slab.
// SAFETY: (contract) `slab` is the start of a live slab – see `# Safety` above.
unsafe fn head(slab: u64) -> &'static mut Head {
    &mut *(slab as *mut Head)
}

impl Slabs {
    /// # Safety
    /// `slab` must be a live slab of class `class` that is not in the list.
    // SAFETY: (contract) a live slab of this class that is not in the list – see `# Safety` above.
    unsafe fn link(&mut self, class: usize, slab: u64) {
        let first = self.partial[class];
        let h = head(slab);
        h.next = first;
        h.prev = 0;
        if first != 0 {
            head(first).prev = slab;
        }
        self.partial[class] = slab;
    }

    /// # Safety
    /// `slab` must be in the list of class `class`.
    // SAFETY: (contract) the slab is in this class's list – see `# Safety` above.
    unsafe fn unlink(&mut self, class: usize, slab: u64) {
        let (next, prev) = (head(slab).next, head(slab).prev);
        if prev != 0 {
            head(prev).next = next;
        } else {
            self.partial[class] = next;
        }
        if next != 0 {
            head(next).prev = prev;
        }
    }

    /// A new slab of the class with every block free, linked into the class's list.
    fn grow(&mut self, class: usize, keep_reserve: bool) -> Option<u64> {
        let pages = slab_pages(class);
        let slab = crate::mem::alloc_slab(pages, class as u16, keep_reserve)?;
        let size = CLASSES[class] as u64;
        let end = slab + pages * PAGE;
        // SAFETY: the pages were just taken from the page allocator and belong to this slab alone;
        // every block lies inside them.
        unsafe {
            let mut free = 0u64;
            let mut block = slab + HEAD + (end - slab - HEAD) / size * size;
            while block > slab + HEAD {
                block -= size;
                (block as *mut u64).write(free);
                free = block;
            }
            (slab as *mut Head).write(Head { free, used: 0, class: class as u32, next: 0, prev: 0 });
            self.link(class, slab);
        }
        self.held += pages * PAGE;
        Some(slab)
    }
}

/// A block of class `class`; `None` if no page can be had (`keep_reserve`: do not go below the
/// kernel reserve for it).
pub fn alloc(class: usize, keep_reserve: bool) -> Option<NonNull<u8>> {
    let mut s = SLABS.lock();
    let slab = match s.partial[class] {
        0 => s.grow(class, keep_reserve)?,
        slab => slab,
    };
    // SAFETY: `slab` is in the class's list, so it is live and has a free block; the block's first
    // word holds the next free block.
    unsafe {
        let h = head(slab);
        let block = h.free;
        h.free = (block as *const u64).read();
        h.used += 1;
        if h.free == 0 {
            s.unlink(class, slab);
        }
        s.used += CLASSES[class] as u64;
        NonNull::new(block as *mut u8)
    }
}

/// Give a block back. `class` comes from the page record of the block's address.
///
/// # Safety
/// `block` must come from [`alloc`] with the same class and not have been freed since.
// SAFETY: (contract) a block from `alloc` of this class, freed once – see `# Safety` above.
pub unsafe fn free(block: NonNull<u8>, class: usize) {
    if class >= CLASSES.len() {
        return;
    }
    let pages = slab_pages(class);
    let addr = block.as_ptr() as u64;
    let slab = addr & !(pages * PAGE - 1);
    let mut s = SLABS.lock();
    let h = head(slab);
    let was_full = h.free == 0;
    (addr as *mut u64).write(h.free);
    h.free = addr;
    h.used -= 1;
    s.used -= CLASSES[class] as u64;
    if was_full {
        s.link(class, slab);
    }
    // an empty slab goes back to the page allocator, unless it is the class's only one with room
    // (one object allocated and freed in a loop must not take and return a page each time)
    if h.used == 0 && !(s.partial[class] == slab && h.next == 0) {
        s.unlink(class, slab);
        s.held -= pages * PAGE;
        drop(s);
        crate::mem::free_slab(slab, pages);
    }
}

/// (bytes held in slabs, bytes handed out as blocks)
pub fn stats() -> (u64, u64) {
    let s = SLABS.lock();
    (s.held, s.used)
}
