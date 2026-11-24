//! The global graphics translation table (GGTT): the display engine reads scan-out buffers through
//! it. It lives in the second half of BAR0 (offset 8 MiB, one 64-bit entry per 4 KiB page). The
//! driver maps kernel pages into a window that the firmware does not use and copies the cache
//! attribute bits (PAT index on display 14) from the firmware's own scan-out entry.

use crate::error::{Error, Result};
use crate::hw::Hw;
use alloc::vec::Vec;

/// Offset of the GGTT in BAR0 (display 9 and 14: 16 MiB BAR, table in the upper half).
pub const GGTT_OFFSET: u32 = 0x80_0000;
/// `GGC` mirror in MMIO: graphics memory control (GGTT size in bits 7:6).
pub const GGC: u32 = 0x108040;
/// `GFX_FLSH_CNTL`: writing 1 flushes the GGTT write buffer and invalidates the TLBs.
pub const GFX_FLSH_CNTL: u32 = 0x101008;
/// Entry: present.
pub const PTE_PRESENT: u64 = 1;
/// Entry: address bits 45:12.
pub const PTE_ADDR_MASK: u64 = 0x0000_3fff_ffff_f000;
/// Page size.
pub const PAGE: u64 = 4096;

/// Number of GGTT entries from `GGC` (0 when the table is absent).
pub fn entries_from_ggc(ggc: u32) -> u32 {
    match (ggc >> 6) & 3 {
        0 => 0,
        n => (1u32 << n) * (1 << 20) / 8,
    }
}

/// An entry for physical page `phys` with the attribute bits `attrs` (everything outside the
/// address and the present bit is taken from `attrs`).
pub fn pte(phys: u64, attrs: u64) -> Result<u64> {
    if phys & (PAGE - 1) != 0 || phys & !PTE_ADDR_MASK != 0 {
        return Err(Error::Invalid("page address"));
    }
    Ok(phys | (attrs & !PTE_ADDR_MASK & !PTE_PRESENT) | PTE_PRESENT)
}

/// The physical page of an entry, if present.
pub fn pte_page(pte: u64) -> Option<u64> {
    (pte & PTE_PRESENT != 0).then_some(pte & PTE_ADDR_MASK)
}

/// Register offset of the entry for GGTT address `gtt`.
pub fn entry_reg(gtt: u32) -> u32 {
    GGTT_OFFSET + (gtt >> 12) * 8
}

/// Reads one entry.
pub fn read_pte<H: Hw>(hw: &mut H, gtt: u32) -> u64 {
    let r = entry_reg(gtt);
    hw.read(r) as u64 | (hw.read(r + 4) as u64) << 32
}

/// Writes one entry (high half first, so a half-written entry is never present with a stale address).
pub fn write_pte<H: Hw>(hw: &mut H, gtt: u32, val: u64) {
    let r = entry_reg(gtt);
    hw.write_gtt(r + 4, (val >> 32) as u32);
    hw.write_gtt(r, val as u32);
}

/// Maps `pages` (physical, in order) at GGTT address `gtt` and invalidates the TLBs. Every entry is
/// read back: a mismatch means the table did not take the write (`Error::DeviceGone`).
pub fn map<H: Hw>(hw: &mut H, gtt: u32, pages: &[u64], attrs: u64) -> Result<()> {
    for (i, p) in pages.iter().enumerate() {
        write_pte(hw, gtt + (i as u32) * PAGE as u32, pte(*p, attrs)?);
    }
    for (i, p) in pages.iter().enumerate() {
        if pte_page(read_pte(hw, gtt + (i as u32) * PAGE as u32)) != Some(*p) {
            return Err(Error::DeviceGone);
        }
    }
    hw.write(GFX_FLSH_CNTL, 1);
    hw.read(GFX_FLSH_CNTL);
    Ok(())
}

/// Points `count` entries from `gtt` on at the scratch page (`scratch`, a PTE from [`pte`]) and
/// invalidates the TLBs. As in i915, an unused entry never holds 0 or a stale address: the
/// display prefetches past the end of a surface, and older parts ignore the valid bit.
pub fn unmap<H: Hw>(hw: &mut H, gtt: u32, count: u32, scratch: u64) {
    for i in 0..count {
        write_pte(hw, gtt.wrapping_add(i.wrapping_mul(PAGE as u32)), scratch);
    }
    hw.write(GFX_FLSH_CNTL, 1);
    hw.read(GFX_FLSH_CNTL);
}

/// Highest GGTT address the window may reach (exclusive): below 4 GiB, page aligned.
const LIMIT: u64 = 0xffff_f000;

/// First-fit allocator over a GGTT window (addresses in bytes, page granular, below 4 GiB).
#[derive(Clone, Debug)]
pub struct Window {
    start: u32,
    end: u32,
    used: Vec<(u32, u32)>,
    max_ranges: usize,
}

impl Window {
    /// A window `[start, end)`; at most `max_ranges` mappings at a time.
    pub fn new(start: u32, end: u32, max_ranges: usize) -> Result<Window> {
        if !start.is_multiple_of(PAGE as u32) || !end.is_multiple_of(PAGE as u32) || start >= end {
            return Err(Error::Invalid("GGTT window"));
        }
        Ok(Window {
            start,
            end,
            used: Vec::new(),
            max_ranges,
        })
    }

    /// The window for a table of `entries` that keeps clear of `[0, fw_end)` (the firmware's
    /// scan-out): the upper half of the table, at most 1 GiB, below 4 GiB.
    pub fn above_firmware(entries: u32, fw_end: u64, max_ranges: usize) -> Result<Window> {
        let top = (entries as u64 * PAGE).min(1 << 32);
        let start = (top / 2).max(
            fw_end
                .checked_next_multiple_of(PAGE)
                .ok_or(Error::Invalid("GGTT window"))?,
        );
        let end = top.min(start.saturating_add(1 << 30)).min(LIMIT);
        Window::from_u64(start, end, max_ranges)
    }

    /// The window inside the CPU aperture (`aperture` bytes of BAR2), so that buffers can be
    /// written through it: from 16 MiB, or the first MiB above the firmware's scan-out, to the
    /// end of the aperture or of the table (below 4 GiB). Any aperture size (4 GiB with a
    /// resizable BAR included) and any firmware surface end are handled without overflow.
    pub fn in_aperture(
        entries: u32,
        fw_end: u64,
        aperture: u64,
        max_ranges: usize,
    ) -> Result<Window> {
        let start = fw_end
            .checked_next_multiple_of(1 << 20)
            .ok_or(Error::Invalid("GGTT window"))?
            .max(16 << 20);
        let end = aperture.min(entries as u64 * PAGE).min(LIMIT);
        Window::from_u64(start, end, max_ranges)
    }

    fn from_u64(start: u64, end: u64, max_ranges: usize) -> Result<Window> {
        let s = u32::try_from(start).map_err(|_| Error::Invalid("GGTT window"))?;
        let e = u32::try_from(end).map_err(|_| Error::Invalid("GGTT window"))?;
        Window::new(s, e, max_ranges)
    }

    /// Reserves `bytes` (rounded up to pages); returns the GGTT address.
    pub fn alloc(&mut self, bytes: u64) -> Result<u32> {
        if bytes == 0 || self.used.len() >= self.max_ranges {
            return Err(Error::NoSpace);
        }
        let len = u32::try_from(bytes.div_ceil(PAGE) * PAGE).map_err(|_| Error::NoSpace)?;
        self.used.sort_unstable();
        let mut at = self.start;
        for &(s, l) in &self.used {
            if s.saturating_sub(at) >= len {
                break;
            }
            at = at.max(s + l);
        }
        if self.end.saturating_sub(at) < len {
            return Err(Error::NoSpace);
        }
        self.used.push((at, len));
        Ok(at)
    }

    /// Releases the range that starts at `gtt`; returns its length in bytes.
    pub fn free(&mut self, gtt: u32) -> Option<u32> {
        let i = self.used.iter().position(|(s, _)| *s == gtt)?;
        Some(self.used.swap_remove(i).1)
    }

    /// Bytes in use.
    pub fn used_bytes(&self) -> u64 {
        self.used.iter().map(|(_, l)| *l as u64).sum()
    }

    /// Start of the window.
    pub fn start(&self) -> u32 {
        self.start
    }

    /// End of the window (exclusive).
    pub fn end(&self) -> u32 {
        self.end
    }

    /// Pages of the window.
    pub fn pages(&self) -> u32 {
        (self.end - self.start) / PAGE as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;

    #[test]
    fn table_size() {
        assert_eq!(entries_from_ggc(3 << 6), 1 << 20);
        assert_eq!(entries_from_ggc(1 << 6), 1 << 18);
        assert_eq!(entries_from_ggc(0), 0);
    }

    #[test]
    fn entries_keep_attributes() {
        let attrs = (1 << 52) | PTE_PRESENT | 0x5000; // PAT0 + stale address bits
        let e = pte(0x1_2345_6000, attrs).unwrap();
        assert_eq!(e, 0x1_2345_6000 | (1 << 52) | 1);
        assert_eq!(pte_page(e), Some(0x1_2345_6000));
        assert_eq!(pte_page(0x1000), None);
        assert_eq!(pte(0x1001, 0), Err(Error::Invalid("page address")));
        assert_eq!(pte(1 << 50, 0), Err(Error::Invalid("page address")));
        assert_eq!(entry_reg(0x0175_b000), 0x80_0000 + 0x175b * 8);
    }

    #[test]
    fn map_reads_back_and_flushes() {
        let mut s = Sim::new();
        map(&mut s, 0x4000_0000, &[0x10_0000, 0x20_0000], 0).unwrap();
        assert_eq!(read_pte(&mut s, 0x4000_0000), 0x10_0001);
        assert_eq!(read_pte(&mut s, 0x4000_1000), 0x20_0001);
        assert_eq!(s.writes.last().map(|w| w.0), Some(GFX_FLSH_CNTL));
        let scratch = pte(0x30_0000, 0).unwrap();
        unmap(&mut s, 0x4000_0000, 2, scratch);
        assert_eq!(
            read_pte(&mut s, 0x4000_0000),
            0x30_0001,
            "scratch page, not 0"
        );
        assert_eq!(read_pte(&mut s, 0x4000_1000), 0x30_0001);
    }

    #[test]
    fn map_detects_a_table_that_does_not_take_writes() {
        let mut s = Sim::new();
        let r = entry_reg(0x1000);
        s.on_write(
            r,
            alloc::boxed::Box::new(move |regs, reg, _| {
                regs.insert(reg, 0);
            }),
        );
        assert_eq!(map(&mut s, 0x1000, &[0x5000], 0), Err(Error::DeviceGone));
    }

    #[test]
    fn window_allocation() {
        let mut w = Window::new(0x1000_0000, 0x1001_0000, 3).unwrap();
        let a = w.alloc(5000).unwrap();
        let b = w.alloc(4096).unwrap();
        assert_eq!((a, b), (0x1000_0000, 0x1000_2000));
        assert_eq!(w.used_bytes(), 3 * 4096);
        assert_eq!(w.free(a), Some(8192));
        assert_eq!(w.free(a), None);
        let c = w.alloc(8192).unwrap();
        assert_eq!(c, 0x1000_0000);
        let _ = w.alloc(4096).unwrap();
        assert_eq!(w.alloc(4096), Err(Error::NoSpace)); // range limit
        let mut w2 = Window::new(0, 0x2000, 8).unwrap();
        assert_eq!(w2.alloc(0x3000), Err(Error::NoSpace));
        assert_eq!(w2.alloc(0), Err(Error::NoSpace));
        assert_eq!(
            Window::new(0x10, 0x2000, 1).err(),
            Some(Error::Invalid("GGTT window"))
        );
    }

    #[test]
    fn window_keeps_clear_of_the_firmware() {
        let w = Window::above_firmware(1 << 20, 0x0175_b000 + 8_294_400, 4).unwrap();
        assert_eq!(w.end(), 0xc000_0000);
        assert_eq!(w.start(), 0x8000_0000);
        let w2 = Window::above_firmware(1 << 18, 0x9000_0000, 4).unwrap_err();
        assert_eq!(w2, Error::Invalid("GGTT window"));
        let mut w3 = Window::above_firmware(1 << 18, 0x0100_0000, 4).unwrap();
        assert_eq!(w3.alloc(4096).unwrap(), 0x2000_0000);
        assert!(Window::above_firmware(1 << 20, u64::MAX - 10, 4).is_err());
        // full 32-bit table: the window ends below 4 GiB
        let w4 = Window::above_firmware(u32::MAX, 0, 4).unwrap();
        assert_eq!((w4.start(), w4.end()), (0x8000_0000, 0xc000_0000));
    }

    #[test]
    fn aperture_window_handles_large_apertures_and_high_surfaces() {
        // 256 MiB aperture, firmware surface at 0x0175_b000
        let w = Window::in_aperture(1 << 20, 0x0175_b000 + 8_294_400, 256 << 20, 4).unwrap();
        assert_eq!((w.start(), w.end()), (0x0200_0000, 0x1000_0000));
        // 4 GiB (resizable BAR) aperture: not truncated to 0
        let w = Window::in_aperture(1 << 20, 0, 1 << 32, 4).unwrap();
        assert_eq!((w.start(), w.end()), (16 << 20, 0xffff_f000));
        assert_eq!(w.pages(), (0xffff_f000 - (16 << 20)) / 4096);
        // a firmware surface ending within 1 MiB of 4 GiB: refused, no overflow
        assert!(Window::in_aperture(1 << 20, 0xffff_8000, 1 << 32, 4).is_err());
        assert!(Window::in_aperture(1 << 20, u64::MAX, 1 << 32, 4).is_err());
    }
}
