//! Per-process page tables of the GT (PPGTT): the address space a context's batches and buffers
//! live in. Four levels of 512 entries map 48-bit addresses to 4 KiB pages, like the CPU's.
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
const ADDR: u64 = 0x000f_ffff_ffff_f000;
/// Cache attribute index 3 of Xe_LPG: write-back, and the GT keeps the CPU's caches coherent.
const CACHE_WB_COHERENT: u64 = PAT0 | PAT1;
const ENTRY_FLAGS: u64 = PRESENT | RW | CACHE_WB_COHERENT;

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
            table = if entry & PRESENT != 0 {
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
        check(va, phys.len() as u64)?;
        if phys.iter().any(|p| p & !ADDR != 0) {
            return Err(Error::Invalid("PPGTT page address"));
        }
        for i in 0..phys.len() as u64 {
            if self.lookup(mem, va + i * PAGE).is_some() {
                return Err(Error::Invalid("PPGTT range already mapped"));
            }
        }
        for (i, p) in phys.iter().enumerate() {
            let at = va + i as u64 * PAGE;
            if let Some(leaf) = self.leaf(mem, at, true)? {
                mem.write(leaf, index(at, 0), p | ENTRY_FLAGS);
            }
        }
        Ok(())
    }

    /// Removes the mapping of `pages` pages at `va` (the tables themselves stay). The caller
    /// invalidates the GT's TLB before it reuses the memory.
    pub fn unmap(&mut self, mem: &mut impl Tables, va: u64, pages: u64) -> Result<()> {
        check(va, pages)?;
        for i in 0..pages {
            let at = va + i * PAGE;
            if let Some(leaf) = self.leaf(mem, at, false)? {
                mem.write(leaf, index(at, 0), 0);
            }
        }
        Ok(())
    }

    /// The memory page mapped at `va`, if any.
    pub fn lookup(&mut self, mem: &mut impl Tables, va: u64) -> Option<u64> {
        if va >= ADDRESS_SPACE {
            return None;
        }
        let leaf = self.leaf(mem, va, false).ok()??;
        let entry = mem.read(leaf, index(va, 0));
        (entry & PRESENT != 0).then_some(entry & ADDR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;

    /// Tables in a map; pages are handed out from 0x10_0000 upwards.
    #[derive(Default)]
    struct Mem {
        entries: BTreeMap<(u64, usize), u64>,
        next: u64,
        limit: Option<u64>,
    }

    impl Tables for Mem {
        fn alloc(&mut self) -> Option<u64> {
            if self.limit.is_some_and(|l| self.next >= l) {
                return None;
            }
            self.next += 1;
            Some(0x10_0000 + (self.next - 1) * PAGE)
        }
        fn read(&mut self, table: u64, index: usize) -> u64 {
            self.entries.get(&(table, index)).copied().unwrap_or(0)
        }
        fn write(&mut self, table: u64, index: usize, entry: u64) {
            self.entries.insert((table, index), entry);
        }
    }

    #[test]
    fn maps_through_four_levels() {
        let mut m = Mem::default();
        let mut pt = Ppgtt::new(&mut m).unwrap();
        assert_eq!(pt.root(), 0x10_0000);
        // 0x1_0020_3000: top index 0, then 4, 1, 3
        let va = 4 << 30 | 1 << 21 | 3 << 12;
        pt.map(&mut m, va, &[0xaaaa_0000, 0xbbbb_0000]).unwrap();
        assert_eq!(pt.table_pages().len(), 4);
        assert_eq!(m.read(0x10_0000, 0), 0x10_1000 | 0x1b);
        assert_eq!(m.read(0x10_1000, 4), 0x10_2000 | 0x1b);
        assert_eq!(m.read(0x10_2000, 1), 0x10_3000 | 0x1b);
        assert_eq!(m.read(0x10_3000, 3), 0xaaaa_0000 | 0x1b);
        assert_eq!(m.read(0x10_3000, 4), 0xbbbb_0000 | 0x1b);
        assert_eq!(pt.lookup(&mut m, va + PAGE), Some(0xbbbb_0000));
        assert_eq!(pt.lookup(&mut m, va + 2 * PAGE), None);
        assert_eq!(pt.lookup(&mut m, 0), None);
    }

    #[test]
    fn a_range_across_a_table_boundary_gets_a_second_table() {
        let mut m = Mem::default();
        let mut pt = Ppgtt::new(&mut m).unwrap();
        let va = (1 << 21) - PAGE;
        pt.map(&mut m, va, &[0x1000, 0x2000]).unwrap();
        assert_eq!(pt.table_pages().len(), 5);
        assert_eq!(pt.lookup(&mut m, va), Some(0x1000));
        assert_eq!(pt.lookup(&mut m, va + PAGE), Some(0x2000));
    }

    #[test]
    fn refuses_double_mapping_and_bad_ranges() {
        let mut m = Mem::default();
        let mut pt = Ppgtt::new(&mut m).unwrap();
        pt.map(&mut m, 0x1000, &[0x5000]).unwrap();
        assert_eq!(
            pt.map(&mut m, 0, &[0x6000, 0x7000]),
            Err(Error::Invalid("PPGTT range already mapped"))
        );
        // nothing of the refused range was written
        assert_eq!(pt.lookup(&mut m, 0), None);
        assert!(pt.map(&mut m, 0x1234, &[0x5000]).is_err());
        assert!(pt
            .map(&mut m, ADDRESS_SPACE - PAGE, &[0x5000, 0x6000])
            .is_err());
        assert!(pt.map(&mut m, 0x8000, &[0x5001]).is_err());
        assert!(pt.map(&mut m, ADDRESS_SPACE - PAGE, &[0x5000]).is_ok());
        assert_eq!(pt.lookup(&mut m, ADDRESS_SPACE), None);
    }

    #[test]
    fn unmap_clears_entries_and_allows_a_new_mapping() {
        let mut m = Mem::default();
        let mut pt = Ppgtt::new(&mut m).unwrap();
        pt.map(&mut m, 0x4000, &[0x5000, 0x6000]).unwrap();
        pt.unmap(&mut m, 0x4000, 1).unwrap();
        assert_eq!(pt.lookup(&mut m, 0x4000), None);
        assert_eq!(pt.lookup(&mut m, 0x5000), Some(0x6000));
        pt.map(&mut m, 0x4000, &[0x9000]).unwrap();
        // unmapping what was never mapped changes nothing and adds no table
        let before = pt.table_pages().len();
        pt.unmap(&mut m, 1 << 40, 4).unwrap();
        assert_eq!(pt.table_pages().len(), before);
    }

    #[test]
    fn running_out_of_memory_is_an_error() {
        let mut m = Mem {
            limit: Some(2),
            ..Mem::default()
        };
        let mut pt = Ppgtt::new(&mut m).unwrap();
        assert_eq!(
            pt.map(&mut m, 0, &[0x5000]),
            Err(Error::Invalid("no memory for a page table"))
        );
        let mut m = Mem {
            limit: Some(0),
            ..Mem::default()
        };
        assert!(Ppgtt::new(&mut m).is_err());
    }
}
