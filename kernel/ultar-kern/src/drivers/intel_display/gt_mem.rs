//! Memory the kernel shares with the GuC: kernel pages mapped into the GGTT, and a word view of
//! them for the command buffers. The GT does not see the CPU's caches and the CPU does not see
//! what the GT wrote (found on the Beelink): every write is pushed out to memory, every read
//! drops the cached line first. An uncached mapping will replace this (intel-display.md, I-12).

use super::buffer::flush;
use super::device::Device;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use ultar_intel_display::ggtt;
use ultar_intel_gt::ctb::Shared;

/// Kernel pages mapped into the GGTT for the GuC.
pub struct GgttBuf {
    phys: u64,
    pages: u64,
    /// Where the pages are mapped in the GGTT.
    pub gtt: u32,
}

impl GgttBuf {
    /// Allocates zeroed pages for `bytes` and maps them.
    pub fn new(dev: &mut Device, bytes: u64) -> Result<GgttBuf, String> {
        let pages = bytes.div_ceil(4096).max(1);
        let phys = crate::mem::alloc_pages(pages as usize).ok_or("no memory for a GuC buffer")?;
        let gtt = match dev.win.alloc(pages * 4096) {
            Ok(g) => g,
            Err(e) => {
                crate::mem::free_run(phys, pages);
                return Err(format!("GGTT: {e}"));
            }
        };
        let list: Vec<u64> = (0..pages).map(|i| phys + i * 4096).collect();
        let buf = GgttBuf { phys, pages, gtt };
        // the GT does not look into the CPU's caches: write the zeroes out, and drop the lines so
        // what the GuC writes later is read from memory
        flush(phys, pages * 4096);
        if let Err(e) = ggtt::map(&mut dev.mmio, gtt, &list, dev.pte_attrs) {
            // An entry did not read back as written. Seen in 2 of 16 runs on the Beelink while
            // the GuC was running, and both times every entry was right on a second look: the
            // read was wrong, not the table. Map once more (write, read back, flush).
            let bad = Self::first_bad(dev, gtt, &list);
            if bad.is_none() && ggtt::map(&mut dev.mmio, gtt, &list, dev.pte_attrs).is_ok() {
                crate::kprintln!(
                    "[intel-gt] note: GGTT read-back at {gtt:#x} failed once ({e}), second pass good"
                );
                return Ok(buf);
            }
            buf.release(dev);
            return Err(format!(
                "GGTT: {e}; {}",
                bad.unwrap_or_else(|| "second mapping pass failed as well".into())
            ));
        }
        Ok(buf)
    }

    /// The first entry that does not point at its page, as text.
    fn first_bad(dev: &mut Device, gtt: u32, list: &[u64]) -> Option<String> {
        list.iter().enumerate().find_map(|(i, p)| {
            let read = ggtt::read_pte(&mut dev.mmio, gtt + i as u32 * 4096);
            (ggtt::pte_page(read) != Some(*p)).then(|| {
                format!(
                    "entry {i} of {} at {gtt:#x}: wrote page {p:#x}, read {read:#x}",
                    list.len()
                )
            })
        })
    }

    /// Copies `data` to the start of the buffer and writes it out of the CPU caches.
    pub fn fill(&mut self, data: &[u8]) {
        let n = data.len().min((self.pages * 4096) as usize);
        // SAFETY: the buffer is `pages` identity-mapped kernel pages owned by this object, `n`
        // does not exceed it, and `data` is a separate heap allocation (no overlap).
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), self.phys as *mut u8, n) };
        flush(self.phys, n as u64);
    }

    /// Copies `data` to byte `offset` of the buffer and writes it out of the CPU caches. Data
    /// that does not fit is not written.
    pub fn write_at(&mut self, offset: usize, data: &[u8]) -> bool {
        let fits = offset
            .checked_add(data.len())
            .is_some_and(|end| end as u64 <= self.pages * 4096);
        if fits {
            let at = self.phys + offset as u64;
            // SAFETY: `[at, at + data.len())` lies inside the identity-mapped kernel pages this
            // object owns (checked above), and `data` is a separate allocation (no overlap).
            unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), at as *mut u8, data.len()) };
            flush(at, data.len() as u64);
        }
        fits
    }

    /// A copy of the first `len` bytes (what the GuC wrote there).
    pub fn bytes(&self, len: usize) -> Vec<u8> {
        let n = len.min((self.pages * 4096) as usize);
        // What the GT writes does not reach the CPU's caches: on the Beelink the first five lines
        // of a table still read as the zeroes from before. Dropping the lines first makes the
        // copy come from memory. The "coherent" cache attribute of the GGTT (index 3) does not
        // change that (tried on the Beelink, 25.11.2026).
        flush(self.phys, n as u64);
        let mut out = alloc::vec![0u8; n];
        // SAFETY: the buffer is `pages` identity-mapped kernel pages owned by this object, `n`
        // does not exceed it, and `out` is a fresh heap allocation (no overlap).
        unsafe { core::ptr::copy_nonoverlapping(self.phys as *const u8, out.as_mut_ptr(), n) };
        out
    }

    /// The memory pages of the buffer (to map them into a context's address space as well).
    pub fn pages(&self) -> Vec<u64> {
        (0..self.pages).map(|i| self.phys + i * 4096).collect()
    }

    /// Points the entries at the scratch page again and frees the pages (the caller made sure
    /// the hardware no longer reads them).
    pub fn release(self, dev: &mut Device) {
        ggtt::unmap(&mut dev.mmio, self.gtt, self.pages as u32, dev.scratch);
        dev.win.free(self.gtt);
        crate::mem::free_run(self.phys, self.pages);
    }
}

impl GgttBuf {
    /// The buffer as 32-bit words for the command buffers.
    pub fn words(&self) -> Words {
        Words {
            phys: self.phys,
            count: (self.pages * 4096 / 4) as usize,
        }
    }
}

/// Word access to a [`GgttBuf`] (valid as long as the buffer is not released).
pub struct Words {
    phys: u64,
    count: usize,
}

impl Shared for Words {
    fn read(&mut self, index: usize) -> u32 {
        if index >= self.count {
            return 0;
        }
        let addr = self.phys + index as u64 * 4;
        flush(addr, 4);
        // SAFETY: `index` is inside the buffer's identity-mapped kernel pages (checked above),
        // the address is 4-byte aligned, and the GuC may change the word at any time, hence a
        // volatile load.
        unsafe { core::ptr::read_volatile(addr as *const u32) }
    }

    fn write(&mut self, index: usize, val: u32) {
        if index >= self.count {
            return;
        }
        let addr = self.phys + index as u64 * 4;
        // SAFETY: as in `read`; a volatile store into pages this buffer owns.
        unsafe { core::ptr::write_volatile(addr as *mut u32, val) };
        flush(addr, 4);
    }
}

/// Memory for the page tables of a context's address space: kernel pages, written through to
/// memory like everything the GT reads.
#[derive(Default)]
pub struct TableMem {
    pages: Vec<u64>,
}

impl TableMem {
    /// Frees every table page (the caller made sure no context uses the tables any more).
    pub fn release(self) {
        for p in self.pages {
            crate::mem::free_run(p, 1);
        }
    }
}

impl ultar_intel_gt::ppgtt::Tables for TableMem {
    fn alloc(&mut self) -> Option<u64> {
        // zeroed by the allocator
        let page = crate::mem::alloc_pages(1)?;
        flush(page, 4096);
        self.pages.push(page);
        Some(page)
    }

    fn read(&mut self, table: u64, index: usize) -> u64 {
        if index >= 512 || !self.pages.contains(&table) {
            return 0;
        }
        // SAFETY: `table` is a page this object allocated (checked above) and `index` stays
        // inside it; the address is 8-byte aligned.
        unsafe { core::ptr::read_volatile((table + index as u64 * 8) as *const u64) }
    }

    fn write(&mut self, table: u64, index: usize, entry: u64) {
        if index >= 512 || !self.pages.contains(&table) {
            return;
        }
        let addr = table + index as u64 * 8;
        // SAFETY: as in `read`; a volatile store into a page this object owns.
        unsafe { core::ptr::write_volatile(addr as *mut u64, entry) };
        flush(addr, 8);
    }
}
