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

const GIB: u64 = 1 << 30;
const MIB2: u64 = 2 << 20;

#[test]
fn eight_gigabytes_of_nothing_cost_eight_entries() {
    let mut m = Mem::default();
    let mut pt = Ppgtt::new(&mut m).unwrap();
    // the range Mesa binds first: 8 GiB at 17 GiB
    pt.map_null(&mut m, 17 * GIB, 8 * GIB / PAGE).unwrap();
    assert_eq!(pt.table_pages().len(), 2);
    assert_eq!(m.read(0x10_1000, 17), 0x283);
    assert_eq!(m.read(0x10_1000, 24), 0x283);
    assert_eq!(m.read(0x10_1000, 25), 0);
    assert!(pt.is_null(&mut m, 17 * GIB) && pt.is_null(&mut m, 25 * GIB - PAGE));
    assert!(!pt.is_null(&mut m, 25 * GIB) && !pt.is_null(&mut m, 16 * GIB));
    assert_eq!(pt.lookup(&mut m, 18 * GIB), None);
}

#[test]
fn an_unaligned_null_range_uses_smaller_entries_at_its_ends() {
    let mut m = Mem::default();
    let mut pt = Ppgtt::new(&mut m).unwrap();
    // from one page below 2 MiB to one page above 4 MiB
    pt.map_null(&mut m, MIB2 - PAGE, (MIB2 + 2 * PAGE) / PAGE).unwrap();
    assert!(pt.is_null(&mut m, MIB2 - PAGE) && pt.is_null(&mut m, MIB2) && pt.is_null(&mut m, 2 * MIB2));
    assert!(!pt.is_null(&mut m, MIB2 - 2 * PAGE) && !pt.is_null(&mut m, 2 * MIB2 + PAGE));
    // the middle 2 MiB is one entry of the second level
    assert_eq!(m.read(0x10_2000, 1), 0x283);
}

#[test]
fn memory_and_nothing_do_not_overlap() {
    let mut m = Mem::default();
    let mut pt = Ppgtt::new(&mut m).unwrap();
    pt.map_null(&mut m, GIB, GIB / PAGE).unwrap();
    assert!(pt.map(&mut m, GIB + 5 * PAGE, &[0xaaaa_0000]).is_err());
    assert!(pt.map_null(&mut m, GIB + MIB2, 1).is_err());
    pt.map(&mut m, 0x4000, &[0xaaaa_0000]).unwrap();
    assert!(pt.map_null(&mut m, 0, 8).is_err());
}

#[test]
fn a_hole_in_a_null_range_splits_it_and_takes_memory() {
    let mut m = Mem::default();
    let mut pt = Ppgtt::new(&mut m).unwrap();
    pt.map_null(&mut m, GIB, GIB / PAGE).unwrap();
    let hole = GIB + 3 * MIB2 + 7 * PAGE;
    pt.unmap(&mut m, hole, 2).unwrap();
    assert!(!pt.is_null(&mut m, hole) && !pt.is_null(&mut m, hole + PAGE));
    assert!(pt.is_null(&mut m, hole - PAGE) && pt.is_null(&mut m, hole + 2 * PAGE));
    assert!(pt.is_null(&mut m, GIB) && pt.is_null(&mut m, 2 * GIB - PAGE));
    pt.map(&mut m, hole, &[0xaaaa_0000, 0xbbbb_0000]).unwrap();
    assert_eq!(pt.lookup(&mut m, hole + PAGE), Some(0xbbbb_0000));
    // removing the whole gigabyte clears the memory pages too
    pt.unmap(&mut m, GIB, GIB / PAGE).unwrap();
    assert!(!pt.is_null(&mut m, GIB) && pt.lookup(&mut m, hole).is_none());
    pt.map_null(&mut m, GIB, GIB / PAGE).unwrap();
}

#[test]
fn a_page_gets_the_cache_attribute_asked_for() {
    let mut m = Mem::default();
    let mut pt = Ppgtt::new(&mut m).unwrap();
    pt.map_cached(&mut m, 0x1000, &[0xaaaa_0000], 0).unwrap();
    pt.map_cached(&mut m, 0x2000, &[0xbbbb_0000], 4).unwrap();
    pt.map(&mut m, 0x3000, &[0xcccc_0000]).unwrap();
    assert_eq!(m.read(0x10_3000, 1), 0xaaaa_0000 | 0x3);
    assert_eq!(m.read(0x10_3000, 2), 0xbbbb_0000 | 0x83);
    assert_eq!(m.read(0x10_3000, 3), 0xcccc_0000 | 0x1b);
    assert!(pt.map_cached(&mut m, 0x4000, &[0xdddd_0000], 5).is_err());
}

#[test]
fn two_megabytes_of_contiguous_memory_take_one_entry() {
    let mut m = Mem::default();
    let mut pt = Ppgtt::new(&mut m).unwrap();
    // 2 MiB at 6 MiB (aligned) from memory at 0x4000_0000, then one more page
    let phys: Vec<u64> = (0..513).map(|k| 0x4000_0000 + k * PAGE).collect();
    assert_eq!(pt.map_cached(&mut m, 3 * MIB2, &phys, 1), Ok(1));
    // root, one table for the 1 GiB, one for the 2 MiB entries, one page table for the last page
    assert_eq!(pt.table_pages().len(), 4);
    assert_eq!(m.read(0x10_2000, 3), 0x4000_0000 | 0x80 | 0xb);
    assert_eq!(pt.lookup(&mut m, 3 * MIB2 + 5 * PAGE), Some(0x4000_5000));
    assert_eq!(pt.lookup(&mut m, 4 * MIB2), Some(0x4020_0000));
    // cache attribute 4 puts its third bit at bit 12 in a 2 MiB entry
    pt.map_cached(&mut m, 5 * MIB2, &phys[..512], 4).unwrap();
    assert_eq!(m.read(0x10_2000, 5), 0x4000_0000 | 0x1000 | 0x83);
    // not aligned in memory: page entries
    pt.map_cached(&mut m, 8 * MIB2, &phys[1..513], 3).unwrap();
    assert_eq!(m.read(0x10_2000, 8) & 0x80, 0);
}

#[test]
fn removing_part_of_a_two_megabyte_entry_keeps_the_rest() {
    let mut m = Mem::default();
    let mut pt = Ppgtt::new(&mut m).unwrap();
    let phys: Vec<u64> = (0..512).map(|k| 0x4000_0000 + k * PAGE).collect();
    pt.map_cached(&mut m, MIB2, &phys, 4).unwrap();
    pt.unmap(&mut m, MIB2 + 10 * PAGE, 1).unwrap();
    assert_eq!(pt.lookup(&mut m, MIB2 + 10 * PAGE), None);
    assert_eq!(pt.lookup(&mut m, MIB2 + 11 * PAGE), Some(0x4000_b000));
    // the pages keep the cache attribute (bit 2 now at bit 7)
    let leaf = m.read(0x10_2000, 1) & 0x000f_ffff_ffff_f000;
    assert_eq!(m.read(leaf, 9), 0x4000_9000 | 0x83);
    pt.unmap(&mut m, MIB2, 512).unwrap();
    assert_eq!(pt.lookup(&mut m, MIB2), None);
    pt.map_cached(&mut m, MIB2, &phys, 0).unwrap();
}
