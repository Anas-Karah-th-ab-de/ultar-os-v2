//! Tests of the target layout (`install.rs`).

use super::*;
use crate::gpt::Header;

fn source() -> Source {
    Source {
        esp: (2048, 4095),
        kernel: (4096, 8191),
        system: (8192, 40959),
        data: (40960, 49151),
    }
}

#[test]
fn plan_has_the_ab_layout_on_mib_boundaries() {
    let p = plan(&source());
    let names: std::vec::Vec<&str> = parts(&p).map(|t| t.name).collect();
    assert_eq!(
        names,
        [
            "ULTAR-EFI",
            "ULTAR-BOOTCTL",
            "ultar-kernel-a",
            "ultar-kernel-b",
            "ultar-system-a",
            "ultar-system-b",
            "ultar-data"
        ]
    );
    for t in parts(&p) {
        assert_eq!(t.first % MIB, 0, "{}", t.name);
    }
    assert_eq!(p[2].sectors, 32 * MIB, "kernel slot at least 32 MiB");
    assert_eq!(p[4].sectors, 64 * MIB, "system slot at least 64 MiB");
    assert_eq!(p[2].fill, Fill::Copy((4096, 8191)));
    assert_eq!(p[3].fill, Fill::Zero);
    assert_eq!(needed(&p), p[6].first + p[6].sectors + 34);
}

#[test]
fn source_needs_all_four_partitions() {
    let s = Source::find(|n| (n != "ultar-kernel").then_some((1, 2)));
    assert_eq!(s, Err(SourceError::Missing("ultar-kernel")));
    assert_eq!(
        Source::find(|_| Some((5, 2))),
        Err(SourceError::Inverted("ULTAR-EFI"))
    );
    assert!(Source::find(|_| Some((1, 2))).is_ok());
}

#[test]
fn gpt_round_trips_and_refuses_small_disks() {
    let p = plan(&source());
    let n = needed(&p) + 100;
    let (mut front, mut back) = ([0u8; FRONT_BYTES], [0u8; BACK_BYTES]);
    let guids = [[7u8; 16]; PARTS];
    assert_eq!(
        gpt(&p, needed(&p) - 1, &guids, [1; 16], &mut front, &mut back),
        None
    );
    let back_lba = gpt(&p, n, &guids, [1; 16], &mut front, &mut back).unwrap();
    assert_eq!(back_lba, n - 33);
    assert_eq!(&front[510..512], &[0x55, 0xAA]);
    let h = Header::parse(&front[512..1024], 1).unwrap();
    h.check_entries(&front[1024..1024 + h.entries_len()])
        .unwrap();
    let e = h.find(&front[1024..], "ultar-data").unwrap().unwrap();
    assert_eq!(e.first_lba, p[6].first);
    let hb = Header::parse(&back[32 * 512..], n - 1).unwrap();
    assert_eq!(hb.entries_lba, n - 33);
}

#[test]
fn regions_hold_the_ab_layout_or_say_why_not() {
    let src = source();
    let need = minimum(&src);
    let mut r = Regions {
        esp: (MIB, need[0]),
        slot_a: (MIB + need[0], need[1]),
        slot_b: (MIB + need[0] + need[1], need[2] + 4 * MIB),
        data: (400 * MIB, 8 * MIB),
    };
    let p = plan_in(&src, &r).unwrap();
    assert_eq!(p[1].first, r.slot_a.0);
    assert_eq!(p[2].first, p[1].first + p[1].sectors);
    assert_eq!(p[4].first + p[4].sectors, r.slot_a.0 + r.slot_a.1);
    assert_eq!(p[5].first + p[5].sectors, r.slot_b.0 + r.slot_b.1);
    assert_eq!((p[6].first, p[6].sectors), r.data);
    r.data.1 = need[3] - 1;
    assert_eq!(
        plan_in(&src, &r),
        Err(RegionError::TooSmall("ultar-data", need[3], need[3] - 1))
    );
    r.data = (r.slot_b.0, need[3]);
    assert_eq!(
        plan_in(&src, &r),
        Err(RegionError::Overlap("slot-b", "ultar-data"))
    );
    r.data = (400 * MIB + 1, need[3]);
    assert_eq!(plan_in(&src, &r), Err(RegionError::Unaligned("ultar-data")));
}

#[test]
fn beside_uses_only_the_free_region_and_its_share() {
    let src = source();
    let need = minimum(&src);
    let total: u64 = need.iter().sum();
    assert!(
        plan_beside(&src, 1000 * MIB + 5, total, 100).is_none(),
        "alignment eats space"
    );
    let free = 4 * total;
    let p = plan_beside(&src, 1000 * MIB + 5, free, 50).unwrap();
    for t in parts(&p) {
        assert!(t.first >= 1000 * MIB + 5 && t.first + t.sectors <= 1000 * MIB + 5 + free);
    }
    let used: u64 = parts(&p).map(|t| t.sectors).sum();
    assert!(used <= free / 2 + MIB && used >= need[0] + need[1] + need[2] + need[3]);
    assert!(
        plan_beside(&src, MIB, free, 0).is_some(),
        "at least the minimum"
    );
}

#[test]
fn gpt_keeping_keeps_foreign_entries_and_refuses_overlap() {
    let src = source();
    let need = minimum(&src);
    let total: u64 = need.iter().sum();
    let n = 2 * MIB + 2 * total + 4 * MIB;
    let mut kept = [[0u8; ENTRY_SIZE]; ENTRY_COUNT];
    build_entry(
        &mut kept[0],
        TYPE_ESP,
        [9; 16],
        MIB,
        2 * MIB - 1,
        "EFI system",
    );
    let p = plan_beside(&src, 2 * MIB, n - 34 - 2 * MIB, 100).unwrap();
    let (mut front, mut back) = ([0u8; FRONT_BYTES], [0u8; BACK_BYTES]);
    let guids = [[7u8; 16]; PARTS];
    assert_eq!(
        gpt_keeping(&p, &kept, n, &guids, [1; 16], &mut front, &mut back),
        Some(n - 33)
    );
    let h = Header::parse(&front[512..1024], 1).unwrap();
    h.check_entries(&front[1024..1024 + h.entries_len()])
        .unwrap();
    assert_eq!(&front[1024..1024 + ENTRY_SIZE], &kept[0][..]);
    assert!(h.find(&front[1024..], "ultar-data").unwrap().is_some());
    build_entry(&mut kept[5], TYPE_LINUX, [8; 16], 2 * MIB, 3 * MIB, "other");
    assert_eq!(
        gpt_keeping(&p, &kept, n, &guids, [1; 16], &mut front, &mut back),
        None
    );
}

#[test]
fn bootctl_image_holds_the_factory_table_twice() {
    let mut small = [0u8; 16];
    assert!(!bootctl_image(&mut small));
    let mut img = std::vec![0xffu8; 1024 * 1024];
    assert!(bootctl_image(&mut img));
    let t = SlotTable::factory().encode();
    assert_eq!(&img[..t.len()], &t[..]);
    assert_eq!(&img[32 * 1024..32 * 1024 + t.len()], &t[..]);
    assert!(img[64 * 1024..].iter().all(|b| *b == 0));
}
