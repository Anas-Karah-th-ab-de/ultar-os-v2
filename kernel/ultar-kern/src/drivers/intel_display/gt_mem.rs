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
            buf.release(dev);
            return Err(format!("GGTT: {e}"));
        }
        Ok(buf)
    }

    /// Copies `data` to the start of the buffer and writes it out of the CPU caches.
    pub fn fill(&mut self, data: &[u8]) {
        let n = data.len().min((self.pages * 4096) as usize);
        // SAFETY: the buffer is `pages` identity-mapped kernel pages owned by this object, `n`
        // does not exceed it, and `data` is a separate heap allocation (no overlap).
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), self.phys as *mut u8, n) };
        flush(self.phys, n as u64);
    }

    /// A copy of the first `len` bytes (what the GuC wrote there).
    pub fn bytes(&self, len: usize) -> Vec<u8> {
        let n = len.min((self.pages * 4096) as usize);
        // What the GT writes does not reach the CPU's caches: on the Beelink the first five lines
        // of the table still read as the zeroes from before. Dropping the lines first makes the
        // copy come from memory.
        flush(self.phys, n as u64);
        let mut out = alloc::vec![0u8; n];
        // SAFETY: the buffer is `pages` identity-mapped kernel pages owned by this object, `n`
        // does not exceed it, and `out` is a fresh heap allocation (no overlap).
        unsafe { core::ptr::copy_nonoverlapping(self.phys as *const u8, out.as_mut_ptr(), n) };
        out
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
