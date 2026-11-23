use super::*;

fn layout_in(dir: &Path) {
    for (name, size) in [
        (BOOTCTL_PART, layout::BOOTCTL_MIN_BYTES),
        ("ultar-kernel-a", 4096),
        ("ultar-kernel-b", 4096),
        ("ultar-system-a", 8192),
        ("ultar-system-b", 8192),
    ] {
        let f = File::create(dir.join(name)).unwrap();
        f.set_len(size).unwrap();
    }
    PartitionBootControl::format(dir).unwrap();
}

#[test]
fn part_names_follow_the_layout() {
    assert_eq!(
        PartitionBootControl::part_name(Role::Kernel, Slot::B).unwrap(),
        "ultar-kernel-b"
    );
    assert_eq!(
        PartitionBootControl::part_name(Role::System, Slot::A).unwrap(),
        "ultar-system-a"
    );
    assert!(PartitionBootControl::part_name(Role::Boot, Slot::A).is_err());
}

#[test]
fn factory_state_and_writes_go_in_place() {
    let dir = tempfile::tempdir().unwrap();
    layout_in(dir.path());
    let mut c =
        PartitionBootControl::open(dir.path(), Slot::A, &[Role::System, Role::Kernel]).unwrap();
    assert_eq!(c.table().unwrap(), SlotTable::factory());
    assert_eq!(c.roles(), [Role::Kernel, Role::System]);
    assert_eq!(c.active_slot().unwrap(), Slot::A);
    let mut w = c.open_writer(Slot::B, Role::System, 5).unwrap();
    w.write_all(b"hello").unwrap();
    w.flush().unwrap();
    w.finish().unwrap();
    let mut back = Vec::new();
    c.open_reader(Slot::B, Role::System)
        .unwrap()
        .take(5)
        .read_to_end(&mut back)
        .unwrap();
    assert_eq!(back, b"hello");
    assert_eq!(
        std::fs::metadata(dir.path().join("ultar-system-b"))
            .unwrap()
            .len(),
        8192,
        "a partition never changes its size"
    );
    c.mark_bootable(Slot::B, &SlotMeta::default()).unwrap();
    assert!(
        c.open_writer(Slot::B, Role::System, 5).is_err(),
        "bootable slot is not written"
    );
    c.set_next_boot_once(Slot::B, 2).unwrap();
    assert_eq!(c.slot_state(Slot::B).unwrap().trial_tries, Some(2));
    assert_eq!(c.table().unwrap().seq, 3, "two changes, two writes");
    c.set_next_boot_once(Slot::B, 2).unwrap();
    assert_eq!(
        c.table().unwrap().seq,
        3,
        "an unchanged table is not written"
    );
    assert_eq!(
        PartitionBootControl::loader_start(dir.path()).unwrap(),
        Slot::B
    );
    let mut b = PartitionBootControl::open(dir.path(), Slot::B, &[Role::System]).unwrap();
    b.mark_successful().unwrap();
    let t = b.table().unwrap();
    assert_eq!(
        (t.default, t.trial, t.slot(Slot::B).successful),
        (Slot::B, None, true)
    );
    b.set_default(Slot::A).unwrap();
    assert_eq!(b.table().unwrap().default, Slot::A);
    assert!(matches!(
        b.open_reader(Slot::A, Role::Kernel),
        Err(UpdateError::InvalidArgument)
    ));
}

#[test]
fn running_slot_file_tolerates_white_space() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("slot");
    std::fs::write(&f, " b \n").unwrap();
    assert_eq!(read_running_slot(&f).unwrap(), Slot::B);
    std::fs::write(&f, "ab").unwrap();
    assert!(matches!(
        read_running_slot(&f),
        Err(UpdateError::BootControl)
    ));
    std::fs::write(&f, vec![b'a'; 100]).unwrap();
    assert!(read_running_slot(&f).is_err(), "oversized file refused");
}
