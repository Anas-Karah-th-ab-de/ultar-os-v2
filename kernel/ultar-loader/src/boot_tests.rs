use super::*;
use crate::disk::{find_parts, LayoutError};
use ultar_slots::gpt::{build_entry, build_header, GptError, Header};
use ultar_slots::layout::{self, BOOTCTL_PART};
use ultar_slots::{crc32, SlotFlags};

/// An in-memory disk with a write cache that only `flush` makes durable, and faults.
pub struct MemDisk {
    pub bs: usize,
    pub durable: Vec<u8>,
    pub cache: Vec<(u64, Vec<u8>)>,
    pub fail_writes: bool,
    pub fail_flush: bool,
    pub flushes: usize,
}

impl Disk for MemDisk {
    fn block_size(&self) -> usize {
        self.bs
    }
    fn last_block(&self) -> u64 {
        (self.durable.len() / self.bs) as u64 - 1
    }
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), DiskError> {
        let at = lba as usize * self.bs;
        buf.copy_from_slice(self.durable.get(at..at + buf.len()).ok_or(DiskError)?);
        for (l, data) in &self.cache {
            // reads see cached writes (like a real drive)
            let c = *l as usize * self.bs;
            for (i, b) in data.iter().enumerate() {
                if (at..at + buf.len()).contains(&(c + i)) {
                    buf[c + i - at] = *b;
                }
            }
        }
        Ok(())
    }
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), DiskError> {
        if self.fail_writes {
            return Err(DiskError);
        }
        self.cache.push((lba, buf.to_vec()));
        Ok(())
    }
    fn flush(&mut self) -> Result<(), DiskError> {
        if self.fail_flush {
            return Err(DiskError);
        }
        for (lba, data) in self.cache.drain(..) {
            let at = lba as usize * self.bs;
            self.durable[at..at + data.len()].copy_from_slice(&data);
        }
        self.flushes += 1;
        Ok(())
    }
}

const MIB: u64 = 1024 * 1024;

/// A disk of 64 MiB with ULTAR-BOOTCTL (1 MiB) and two kernel partitions (8 MiB each).
pub fn disk(bs: usize, bootctl_bytes: u64) -> MemDisk {
    let blocks = (64 * MIB) as usize / bs;
    let mut d = MemDisk {
        bs,
        durable: vec![0u8; blocks * bs],
        cache: Vec::new(),
        fail_writes: false,
        fail_flush: false,
        flushes: 0,
    };
    let b = |bytes: u64| bytes / bs as u64;
    let mut entries = vec![0u8; 128 * 128];
    let parts = [
        (BOOTCTL_PART, b(MIB), b(MIB + bootctl_bytes) - 1),
        ("ultar-kernel-a", b(4 * MIB), b(12 * MIB) - 1),
        ("ultar-kernel-b", b(12 * MIB), b(20 * MIB) - 1),
    ];
    for (i, (name, first, last)) in parts.iter().enumerate() {
        build_entry(
            &mut entries[i * 128..],
            [0xAF; 16],
            [i as u8 + 1; 16],
            *first,
            *last,
            name,
        );
    }
    let ent_blocks = (entries.len() / bs) as u64;
    let last = blocks as u64 - 1;
    let mut h = Header {
        my_lba: 1,
        alternate_lba: last,
        first_usable: 2 + ent_blocks,
        last_usable: last - 1 - ent_blocks,
        entries_lba: 2,
        entry_count: 128,
        entry_size: 128,
        entries_crc: crc32(&entries),
        disk_guid: [9; 16],
    };
    let primary = build_header(&h, &entries);
    d.durable[bs..bs + 512].copy_from_slice(&primary);
    d.durable[2 * bs..2 * bs + entries.len()].copy_from_slice(&entries);
    h.my_lba = last;
    h.alternate_lba = 1;
    h.entries_lba = last - ent_blocks;
    let backup = build_header(&h, &entries);
    let at = h.entries_lba as usize * bs;
    d.durable[at..at + entries.len()].copy_from_slice(&entries);
    d.durable[last as usize * bs..last as usize * bs + 512].copy_from_slice(&backup);
    let table = SlotTable::factory().encode();
    for off in TABLE_COPY_OFFSETS {
        let at = (MIB + off) as usize;
        d.durable[at..at + 64].copy_from_slice(&table);
    }
    d
}

fn set_table(d: &mut MemDisk, t: SlotTable) {
    let at = (MIB + TABLE_COPY_OFFSETS[1]) as usize;
    let mut t = t;
    t.seq = 50;
    d.durable[at..at + 64].copy_from_slice(&t.encode());
}

fn on_disk(d: &mut MemDisk) -> SlotTable {
    let mut scratch = vec![0u8; 64 * 1024];
    let parts = find_parts(d, &mut scratch).unwrap();
    let mut block = vec![0u8; d.bs];
    SlotTable::newest(read_copies(d, &parts, &mut block)).unwrap()
}

fn trial_b() -> SlotTable {
    let mut t = SlotTable::factory();
    t.flags[1] = SlotFlags {
        bootable: true,
        successful: false,
    };
    t.trial = Some((Slot::B, 3));
    t
}

fn decide_on(d: &mut MemDisk) -> Result<Start, Rescue> {
    let mut scratch = vec![0u8; 64 * 1024];
    let parts = find_parts(d, &mut scratch).unwrap();
    let mut block = vec![0u8; d.bs];
    decide(d, &parts, &mut block)
}

#[test]
fn factory_disk_starts_a_without_writing() {
    for bs in [512, 4096] {
        let mut d = disk(bs, MIB);
        assert_eq!(
            decide_on(&mut d),
            Ok(Start {
                slot: Slot::A,
                trial: false,
                stored: true
            })
        );
        assert_eq!(d.flushes, 0, "nothing changed, nothing written");
    }
}

#[test]
fn trial_counts_down_durably_then_falls_back() {
    for bs in [512, 4096] {
        let mut d = disk(bs, MIB);
        set_table(&mut d, trial_b());
        for left in [2u8, 1, 0] {
            assert_eq!(
                decide_on(&mut d),
                Ok(Start {
                    slot: Slot::B,
                    trial: true,
                    stored: true
                })
            );
            assert!(d.cache.is_empty(), "flushed before the start");
            assert_eq!(on_disk(&mut d).trial, Some((Slot::B, left)));
        }
        assert_eq!(
            decide_on(&mut d),
            Ok(Start {
                slot: Slot::A,
                trial: false,
                stored: true
            })
        );
        let t = on_disk(&mut d);
        assert_eq!(
            (t.trial, t.slot(Slot::B).bootable, t.last_booted),
            (None, false, Some(Slot::A))
        );
    }
}

#[test]
fn a_trial_that_cannot_be_recorded_is_not_started() {
    let mut d = disk(512, MIB);
    set_table(&mut d, trial_b());
    d.fail_writes = true;
    assert_eq!(
        decide_on(&mut d),
        Ok(Start {
            slot: Slot::A,
            trial: false,
            stored: false
        })
    );
    d.fail_writes = false;
    d.fail_flush = true;
    assert_eq!(
        decide_on(&mut d),
        Ok(Start {
            slot: Slot::A,
            trial: false,
            stored: false
        })
    );
    d.fail_flush = false;
    d.cache.clear(); // power loss: the unflushed write is gone
    assert_eq!(
        on_disk(&mut d).trial,
        Some((Slot::B, 3)),
        "no attempt was used"
    );
}

#[test]
fn torn_write_keeps_the_current_copy() {
    let mut d = disk(512, MIB);
    set_table(&mut d, trial_b());
    // power loss in the middle of the write of copy 0 (the non-current one)
    let at = (MIB + TABLE_COPY_OFFSETS[0]) as usize;
    d.durable[at + 10..at + 40].fill(0xEE);
    assert_eq!(on_disk(&mut d).trial, Some((Slot::B, 3)));
    assert_eq!(decide_on(&mut d).unwrap().slot, Slot::B);
    assert_eq!(on_disk(&mut d).trial, Some((Slot::B, 2)));
}

#[test]
fn start_loop_spends_the_tries_then_starts_the_old_slot() {
    let mut d = disk(512, MIB);
    set_table(&mut d, trial_b());
    let mut scratch = vec![0u8; 64 * 1024];
    let parts = find_parts(&mut d, &mut scratch).unwrap();
    let mut block = vec![0u8; 512];
    let mut seen = Vec::new();
    let r = run(&mut d, &parts, &mut block, &mut |_, s| {
        seen.push((s.slot, s.trial))
    });
    assert_eq!(
        seen,
        [
            (Slot::B, true),
            (Slot::B, true),
            (Slot::B, true),
            (Slot::A, false)
        ]
    );
    assert_eq!(r, Rescue::KernelUnusable(Slot::A));
    assert!(!on_disk(&mut d).slot(Slot::B).bootable);
}

#[test]
fn rescue_cases() {
    let mut d = disk(512, MIB);
    for off in TABLE_COPY_OFFSETS {
        let at = (MIB + off) as usize;
        d.durable[at] ^= 1;
    }
    assert_eq!(decide_on(&mut d), Err(Rescue::NoTable));
    let mut d = disk(512, MIB);
    let mut t = SlotTable::factory();
    t.flags = [SlotFlags::default(); 2];
    set_table(&mut d, t);
    assert_eq!(decide_on(&mut d), Err(Rescue::NoBootableSlot));
    let mut scratch = vec![0u8; 64 * 1024];
    let parts = find_parts(&mut d, &mut scratch).unwrap();
    let mut block = vec![0u8; 512];
    assert_eq!(
        run(&mut d, &parts, &mut block, &mut |_, _| panic!(
            "nothing may start"
        )),
        Rescue::NoBootableSlot
    );
}

#[test]
fn partition_table_checks() {
    let mut scratch = vec![0u8; 64 * 1024];
    let mut d = disk(512, MIB);
    let p = find_parts(&mut d, &mut scratch).unwrap();
    assert_eq!(p.bootctl.first_lba, 2048);
    assert_eq!(p.kernel(Slot::B).unwrap().first_lba, 12 * 2048);
    assert_eq!(p.disk_guid, [9; 16]);
    // damaged primary header: the backup table is used
    d.durable[512 + 30] ^= 0xFF;
    assert_eq!(find_parts(&mut d, &mut scratch).unwrap(), p);
    // both damaged
    let last = d.durable.len() - 512;
    d.durable[last + 30] ^= 0xFF;
    assert!(matches!(
        find_parts(&mut d, &mut scratch),
        Err(LayoutError::Gpt(_))
    ));
    // too small a boot control partition
    let mut d = disk(512, layout::BOOTCTL_MIN_BYTES - 512);
    assert_eq!(
        find_parts(&mut d, &mut scratch),
        Err(LayoutError::NoBootControl)
    );
    let mut d = disk(512, MIB);
    d.bs = 1000;
    assert_eq!(
        find_parts(&mut d, &mut scratch),
        Err(LayoutError::BlockSize)
    );
    // damaged entry array in the primary table only: backup used
    let mut d = disk(512, MIB);
    d.durable[2 * 512 + 60] ^= 1;
    assert!(find_parts(&mut d, &mut scratch).is_ok());
    assert_eq!(GptError::EntryCrc, GptError::EntryCrc);
}

#[test]
fn missing_boot_control_is_reported() {
    let mut d = disk(512, MIB);
    // rename ULTAR-BOOTCTL in both tables → not found
    let mut scratch = vec![0u8; 64 * 1024];
    for lba in [1u64, d.last_block()] {
        let mut hb = vec![0u8; 512];
        d.read(lba, &mut hb).unwrap();
        let mut h = Header::parse(&hb, lba).unwrap();
        let mut entries = vec![0u8; h.entries_len()];
        d.read(h.entries_lba, &mut entries).unwrap();
        entries[56] = b'X';
        h.entries_crc = crc32(&entries);
        let at = h.entries_lba as usize * 512;
        d.durable[at..at + entries.len()].copy_from_slice(&entries);
        let nb = build_header(&h, &entries);
        d.durable[lba as usize * 512..lba as usize * 512 + 512].copy_from_slice(&nb);
    }
    assert_eq!(
        find_parts(&mut d, &mut scratch),
        Err(LayoutError::NoBootControl)
    );
}
