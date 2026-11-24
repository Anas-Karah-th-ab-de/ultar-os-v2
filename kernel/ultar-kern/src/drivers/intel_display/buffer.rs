//! Scan-out buffers in kernel memory: physically contiguous pages, mapped into the GGTT, written
//! by the CPU and flushed out of its caches (the display engine does not snoop them).

use super::device::Device;
use super::mmio::Mmio;
use ultar_intel_display::ggtt::{self, Window};
use ultar_intel_display::Error;

/// One buffer.
pub struct ScanBuf {
    /// Physical (= kernel virtual) address of the first byte.
    pub phys: u64,
    /// Pages.
    pub pages: u64,
    /// GGTT address the display engine uses.
    pub gtt: u32,
    /// Bytes per row.
    pub pitch: u32,
    /// Size in pixels.
    pub width: u32,
    /// Size in pixels.
    pub height: u32,
}

/// Writes all cache lines of `[start, start + len)` back to memory.
pub fn flush(start: u64, len: u64) {
    let mut a = start & !63;
    while a < start + len {
        // SAFETY: `a` lies inside an identity-mapped kernel buffer owned by this driver; clflush
        // only writes the line back and has no other effect on memory contents.
        unsafe { core::arch::x86_64::_mm_clflush(a as *const u8) };
        a += 64;
    }
    // SAFETY: a fence instruction without memory operands.
    unsafe { core::arch::x86_64::_mm_mfence() };
}

impl ScanBuf {
    /// Allocates an XRGB8888 buffer of `width` × `height` and maps it at a free GGTT place (on
    /// failure the entries written so far point at the scratch page `scratch` again).
    pub fn new(
        mmio: &mut Mmio,
        win: &mut Window,
        attrs: u64,
        scratch: u64,
        width: u32,
        height: u32,
    ) -> Result<ScanBuf, Error> {
        let pitch = u32::try_from((width as u64 * 4).next_multiple_of(64))
            .map_err(|_| Error::Invalid("buffer size"))?;
        let bytes = pitch as u64 * height as u64;
        let pages = bytes.div_ceil(4096);
        let phys = crate::mem::alloc_pages(pages as usize).ok_or(Error::NoSpace)?;
        let gtt = match win.alloc(bytes) {
            Ok(g) => g,
            Err(e) => {
                crate::mem::free_run(phys, pages);
                return Err(e);
            }
        };
        let list: alloc::vec::Vec<u64> = (0..pages).map(|i| phys + i * 4096).collect();
        if let Err(e) = ggtt::map(mmio, gtt, &list, attrs) {
            ggtt::unmap(mmio, gtt, pages as u32, scratch);
            win.free(gtt);
            crate::mem::free_run(phys, pages);
            return Err(e);
        }
        Ok(ScanBuf {
            phys,
            pages,
            gtt,
            pitch,
            width,
            height,
        })
    }

    /// Fills the buffer with `f(x, y)` and flushes it.
    pub fn draw(&mut self, f: impl Fn(u32, u32) -> u32) {
        for y in 0..self.height {
            let row = self.phys + y as u64 * self.pitch as u64;
            for x in 0..self.width {
                // SAFETY: (x, y) lies inside the buffer this object owns (pitch × height bytes,
                // identity-mapped kernel memory), 4-byte aligned.
                unsafe { core::ptr::write_volatile((row + x as u64 * 4) as *mut u32, f(x, y)) };
            }
        }
        flush(self.phys, self.pitch as u64 * self.height as u64);
    }

    /// Points the buffer's GGTT entries at the scratch page (`scratch`) and frees it (the caller
    /// made sure the hardware no longer scans it out).
    pub fn release(self, mmio: &mut Mmio, win: &mut Window, scratch: u64) {
        ggtt::unmap(mmio, self.gtt, self.pages as u32, scratch);
        win.free(self.gtt);
        crate::mem::free_run(self.phys, self.pages);
    }
}

impl Device {
    /// Allocates a kernel scan-out buffer of `w`×`h` pixels.
    pub fn new_buffer(&mut self, w: u32, h: u32) -> Result<ScanBuf, Error> {
        ScanBuf::new(
            &mut self.mmio,
            &mut self.win,
            self.pte_attrs,
            self.scratch,
            w,
            h,
        )
    }

    /// Releases a buffer that is not on any plane.
    pub fn free_buffer(&mut self, b: ScanBuf) {
        b.release(&mut self.mmio, &mut self.win, self.scratch);
    }

    /// CPU address of a buffer through the aperture, if it is reachable there.
    pub fn aperture_addr(&self, b: &ScanBuf) -> Option<u64> {
        let (base, size) = self.aperture?;
        let end = b.gtt as u64 + b.pitch as u64 * b.height as u64;
        (end <= size).then_some(base + b.gtt as u64)
    }
}
