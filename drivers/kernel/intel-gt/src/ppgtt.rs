//! Per-process page tables of the GT (PPGTT): the address space a context's batches and buffers
//! live in. Four levels of 512 entries map 48-bit addresses to 4 KiB pages, like the CPU's.
//! A range can also be mapped to *nothing* ("null": writes vanish, reads give zero); such a range
//! uses one entry per 1 GiB or 2 MiB where it is aligned, so gigabytes cost a handful of entries.
//! Entry encoding follows `xe_vm.c` (`xelp_pte_encode_*`) and `regs/xe_gtt_defs.h` of the Linux
//! `xe` driver (MIT).
//!
//! The tables are ordinary memory pages; [`Tables`] is how this module reaches them (the kernel
//! hands out zeroed pages and writes through to memory, the tests use a map).

use crate::{Error, Result};
use alloc::vec::Vec;

/// Size of a page and of a page table.
pub const PAGE: u64 = 4096;
/// Addresses the tables can map (48 bits).
pub const ADDRESS_SPACE: u64 = 1 << 48;

const PRESENT: u64 = 1 << 0;
const RW: u64 = 1 << 1;
const PAT0: u64 = 1 << 3;
const PAT1: u64 = 1 << 4;
/// An entry of a middle level that maps its whole span itself (2 MiB or 1 GiB) – no table below.
const HUGE: u64 = 1 << 7;
/// The entry maps to nothing: writes are dropped, reads return zero (`XE_PTE_NULL`).
const NULL: u64 = 1 << 9;
const ADDR: u64 = 0x000f_ffff_ffff_f000;
/// The highest level whose entries may map their span themselves (1 GiB).
const MAX_HUGE_LEVEL: u32 = 2;
/// Cache attribute index 3 of Xe_LPG: write-back, and the GT keeps the CPU's caches coherent.
const CACHE_WB_COHERENT: u64 = PAT0 | PAT1;
const ENTRY_FLAGS: u64 = PRESENT | RW | CACHE_WB_COHERENT;
/// Bit 2 of a page's cache attribute index (bits 0 and 1 are PAT0, PAT1).
const PAT2: u64 = 1 << 7;
/// Index of write-back with the GT coherent to the CPU's caches.
pub const PAT_WB_COHERENT: u8 = 3;

/// The bits of a page entry (4 KiB) for cache attribute index `pat` (`xelp_pte_encode_pat`).
fn pat_bits(pat: u8) -> u64 {
    let bit = |n: u8, b: u64| if pat & (1 << n) != 0 { b } else { 0 };
    bit(0, PAT0) | bit(1, PAT1) | bit(2, PAT2)
}

/// Access to the pages that hold the tables.
pub trait Tables {
    /// A new zeroed page; `None` when memory is out.
    fn alloc(&mut self) -> Option<u64>;
    /// Reads entry `index` (0..512) of the table at `table`.
    fn read(&mut self, table: u64, index: usize) -> u64;
    /// Writes entry `index` of the table at `table`.
    fn write(&mut self, table: u64, index: usize, entry: u64);
}

/// One address space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ppgtt {
    root: u64,
    pages: Vec<u64>,
}

fn index(va: u64, level: u32) -> usize {
    ((va >> (12 + 9 * level)) & 0x1ff) as usize
}

/// Bytes one entry of a table at `level` covers (level 0: a page).
fn span(level: u32) -> u64 {
    PAGE << (9 * level)
}

/// A null entry for `level`.
fn null_entry(level: u32) -> u64 {
    PRESENT | RW | NULL | if level > 0 { HUGE } else { 0 }
}

/// The entries of a table at `level` that `[start, end)` touches: index, and the part of the
/// range inside that entry.
fn entries(level: u32, start: u64, end: u64) -> impl Iterator<Item = (usize, u64, u64)> {
    let s = span(level);
    let mut at = start;
    core::iter::from_fn(move || {
        if at >= end {
            return None;
        }
        let stop = ((at / s + 1) * s).min(end);
        let item = (index(at, level), at, stop);
        at = stop;
        Some(item)
    })
}

fn check(va: u64, pages: u64) -> Result<()> {
    let end = pages.checked_mul(PAGE).and_then(|len| va.checked_add(len));
    if !va.is_multiple_of(PAGE) || !end.is_some_and(|e| e <= ADDRESS_SPACE) {
        return Err(Error::Invalid("PPGTT address range"));
    }
    Ok(())
}

impl Ppgtt {
    /// An empty address space (one page for the top-level table).
    pub fn new(mem: &mut impl Tables) -> Result<Ppgtt> {
        let root = mem
            .alloc()
            .ok_or(Error::Invalid("no memory for a page table"))?;
        Ok(Ppgtt {
            root,
            pages: alloc::vec![root],
        })
    }

    /// Address of the top-level table (goes into the context image).
    pub fn root(&self) -> u64 {
        self.root
    }

    /// Every page the tables occupy (to give back when the address space ends).
    pub fn table_pages(&self) -> &[u64] {
        &self.pages
    }

    /// The last-level table for `va`; with `create` missing levels are added.
    fn leaf(&mut self, mem: &mut impl Tables, va: u64, create: bool) -> Result<Option<u64>> {
        let mut table = self.root;
        for level in (1..=3).rev() {
            let i = index(va, level);
            let entry = mem.read(table, i);
            table = if entry & PRESENT != 0 && entry & HUGE != 0 {
                // a null range sits here: no table below
                return if create { Err(Error::Invalid("PPGTT range already mapped")) } else { Ok(None) };
            } else if entry & PRESENT != 0 {
                entry & ADDR
            } else if create {
                let next = mem
                    .alloc()
                    .ok_or(Error::Invalid("no memory for a page table"))?;
                self.pages.push(next);
                mem.write(table, i, next | ENTRY_FLAGS);
                next
            } else {
                return Ok(None);
            };
        }
        Ok(Some(table))
    }

    /// Maps the memory pages `phys` (page-aligned addresses) at `va` and following. Refuses a
    /// range that is mapped already.
    pub fn map(&mut self, mem: &mut impl Tables, va: u64, phys: &[u64]) -> Result<()> {
        self.map_cached(mem, va, phys, PAT_WB_COHERENT)
    }

    /// [`Ppgtt::map`] with cache attribute `pat` (index 0–4 of the GT's PAT table; programs name
    /// it in their bind requests).
    pub fn map_cached(&mut self, mem: &mut impl Tables, va: u64, phys: &[u64], pat: u8) -> Result<()> {
        if pat > 4 {
            return Err(Error::Invalid("PPGTT cache attribute"));
        }
        check(va, phys.len() as u64)?;
        if phys.iter().any(|p| p & !ADDR != 0) {
            return Err(Error::Invalid("PPGTT page address"));
        }
        if self.mapped_in(mem, self.root, 3, va, va + phys.len() as u64 * PAGE) {
            return Err(Error::Invalid("PPGTT range already mapped"));
        }
        for (i, p) in phys.iter().enumerate() {
            let at = va + i as u64 * PAGE;
            if let Some(leaf) = self.leaf(mem, at, true)? {
                mem.write(leaf, index(at, 0), p | PRESENT | RW | pat_bits(pat));
            }
        }
        Ok(())
    }

    /// Removes the mapping of `pages` pages at `va` (the tables themselves stay). The caller
    /// invalidates the GT's TLB before it reuses the memory.
    pub fn unmap(&mut self, mem: &mut impl Tables, va: u64, pages: u64) -> Result<()> {
        check(va, pages)?;
        self.clear_in(mem, self.root, 3, va, va + pages * PAGE)
    }

    /// Maps `pages` pages at `va` to nothing: what a job writes there is dropped, what it reads
    /// is zero. Refuses a range that is mapped already.
    pub fn map_null(&mut self, mem: &mut impl Tables, va: u64, pages: u64) -> Result<()> {
        check(va, pages)?;
        let end = va + pages * PAGE;
        if self.mapped_in(mem, self.root, 3, va, end) {
            return Err(Error::Invalid("PPGTT range already mapped"));
        }
        self.fill_null(mem, self.root, 3, va, end)
    }

    /// The table below entry `i` of `table`, made if it is missing.
    fn child(&mut self, mem: &mut impl Tables, table: u64, i: usize) -> Result<u64> {
        let entry = mem.read(table, i);
        if entry & PRESENT != 0 {
            return Ok(entry & ADDR);
        }
        let next = mem.alloc().ok_or(Error::Invalid("no memory for a page table"))?;
        self.pages.push(next);
        mem.write(table, i, next | ENTRY_FLAGS);
        Ok(next)
    }

    /// Whether anything is mapped in `[start, end)` below `table`.
    fn mapped_in(&mut self, mem: &mut impl Tables, table: u64, level: u32, start: u64, end: u64) -> bool {
        entries(level, start, end).any(|(i, from, to)| {
            let entry = mem.read(table, i);
            entry & PRESENT != 0 && (level == 0 || entry & HUGE != 0 || self.mapped_in(mem, entry & ADDR, level - 1, from, to))
        })
    }

    /// Null entries for `[start, end)`: one entry where the range covers an entry's whole span.
    fn fill_null(&mut self, mem: &mut impl Tables, table: u64, level: u32, start: u64, end: u64) -> Result<()> {
        for (i, from, to) in entries(level, start, end) {
            if level <= MAX_HUGE_LEVEL && to - from == span(level) {
                mem.write(table, i, null_entry(level));
            } else {
                let next = self.child(mem, table, i)?;
                self.fill_null(mem, next, level - 1, from, to)?;
            }
        }
        Ok(())
    }

    /// Clears `[start, end)`. A null entry that is only partly inside is first replaced by a
    /// table of smaller null entries.
    fn clear_in(&mut self, mem: &mut impl Tables, table: u64, level: u32, start: u64, end: u64) -> Result<()> {
        for (i, from, to) in entries(level, start, end) {
            let entry = mem.read(table, i);
            if entry & PRESENT == 0 {
                continue;
            }
            if level == 0 || (entry & HUGE != 0 && to - from == span(level)) {
                mem.write(table, i, 0);
                continue;
            }
            let next = if entry & HUGE != 0 {
                let next = mem.alloc().ok_or(Error::Invalid("no memory for a page table"))?;
                self.pages.push(next);
                (0..512).for_each(|k| mem.write(next, k, null_entry(level - 1)));
                mem.write(table, i, next | ENTRY_FLAGS);
                next
            } else {
                entry & ADDR
            };
            self.clear_in(mem, next, level - 1, from, to)?;
        }
        Ok(())
    }

    /// Whether `va` is mapped to nothing.
    pub fn is_null(&mut self, mem: &mut impl Tables, va: u64) -> bool {
        let mut table = self.root;
        for level in (0..=3).rev() {
            let entry = mem.read(table, index(va, level));
            if entry & PRESENT == 0 {
                return false;
            }
            if level == 0 || entry & HUGE != 0 {
                return entry & NULL != 0;
            }
            table = entry & ADDR;
        }
        false
    }

    /// The memory page mapped at `va`, if any.
    pub fn lookup(&mut self, mem: &mut impl Tables, va: u64) -> Option<u64> {
        if va >= ADDRESS_SPACE {
            return None;
        }
        let leaf = self.leaf(mem, va, false).ok()??;
        let entry = mem.read(leaf, index(va, 0));
        (entry & PRESENT != 0 && entry & NULL == 0).then_some(entry & ADDR)
    }
}

#[cfg(test)]
mod tests;
