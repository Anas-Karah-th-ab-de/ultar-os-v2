use super::*;

const ROLES: [Role; 2] = [Role::Kernel, Role::System];

fn control() -> (tempfile::TempDir, FileBootControl) {
    let dir = tempfile::tempdir().unwrap();
    let control = FileBootControl::create(dir.path(), &ROLES).unwrap();
    (dir, control)
}

fn fill(control: &mut FileBootControl, slot: Slot, byte: u8) {
    for role in ROLES {
        let mut writer = control.open_writer(slot, role, 100).unwrap();
        writer.write_all(&[byte; 100]).unwrap();
        writer.finish().unwrap();
    }
}

#[test]
fn factory_state() {
    let (dir, control) = control();
    assert_eq!(control.active_slot().unwrap(), Slot::A);
    let a = control.slot_state(Slot::A).unwrap();
    assert!(a.bootable && a.successful && a.is_default && a.trial_tries.is_none());
    assert_eq!(control.slot_state(Slot::B).unwrap(), SlotState::default());
    assert_eq!(control.roles(), ROLES);
    // create() on an existing control changes nothing; open() needs a table.
    let seq = control.table().unwrap().seq;
    let again = FileBootControl::create(dir.path(), &ROLES).unwrap();
    assert_eq!(again.table().unwrap().seq, seq);
    let empty = tempfile::tempdir().unwrap();
    assert!(matches!(
        FileBootControl::open(empty.path(), &ROLES),
        Err(UpdateError::BootControl)
    ));
}

#[test]
fn full_trial_cycle() {
    let (_dir, mut control) = control();
    fill(&mut control, Slot::B, 7);
    let meta = SlotMeta {
        verity_root: Some([9; 32]),
    };
    control.mark_bootable(Slot::B, &meta).unwrap();
    assert_eq!(control.verity_root(Slot::B).unwrap(), Some([9; 32]));
    control.set_next_boot_once(Slot::B, 3).unwrap();
    assert_eq!(control.slot_state(Slot::B).unwrap().trial_tries, Some(3));
    assert_eq!(control.simulate_boot().unwrap(), Slot::B);
    assert_eq!(control.active_slot().unwrap(), Slot::B);
    assert!(
        control.slot_state(Slot::A).unwrap().is_default,
        "old slot stays default"
    );
    control.mark_successful().unwrap();
    let b = control.slot_state(Slot::B).unwrap();
    assert!(b.successful && b.is_default && b.trial_tries.is_none());
    assert_eq!(control.simulate_boot().unwrap(), Slot::B);
    // Manual return to the previous version.
    control.set_default(Slot::A).unwrap();
    assert_eq!(control.simulate_boot().unwrap(), Slot::A);
}

#[test]
fn fallback_after_failed_starts() {
    let (_dir, mut control) = control();
    fill(&mut control, Slot::B, 7);
    control
        .mark_bootable(Slot::B, &SlotMeta::default())
        .unwrap();
    assert_eq!(control.verity_root(Slot::B).unwrap(), None);
    control.set_next_boot_once(Slot::B, 2).unwrap();
    assert_eq!(control.simulate_boot().unwrap(), Slot::B);
    assert_eq!(control.simulate_boot().unwrap(), Slot::B);
    assert_eq!(control.simulate_boot().unwrap(), Slot::A);
    assert!(!control.slot_state(Slot::B).unwrap().bootable);
    assert!(matches!(
        control.set_default(Slot::B),
        Err(UpdateError::PreviousUnavailable)
    ));
}

#[test]
fn guards_against_touching_the_running_or_a_bootable_slot() {
    let (_dir, mut control) = control();
    assert!(matches!(
        control.mark_unbootable(Slot::A),
        Err(UpdateError::InvalidArgument)
    ));
    assert!(control.open_writer(Slot::A, Role::Kernel, 1).is_err());
    assert!(
        control.open_writer(Slot::B, Role::Boot, 1).is_err(),
        "role not in layout"
    );
    assert!(control
        .mark_bootable(Slot::A, &SlotMeta::default())
        .is_err());
    assert!(
        control.set_next_boot_once(Slot::B, 3).is_err(),
        "not bootable"
    );
    fill(&mut control, Slot::B, 1);
    control
        .mark_bootable(Slot::B, &SlotMeta::default())
        .unwrap();
    assert!(control.set_next_boot_once(Slot::B, 0).is_err());
    assert!(
        control.open_writer(Slot::B, Role::Kernel, 1).is_err(),
        "bootable slot"
    );
    assert!(matches!(
        control.set_default(Slot::B),
        Err(UpdateError::PreviousUnavailable)
    ));
    control.set_next_boot_once(Slot::B, 1).unwrap();
    control.mark_unbootable(Slot::B).unwrap();
    let b = control.slot_state(Slot::B).unwrap();
    assert!(!b.bootable && b.trial_tries.is_none());
}

#[test]
fn writer_is_bounded_by_the_announced_size() {
    let (_dir, mut control) = control();
    let mut writer = control.open_writer(Slot::B, Role::Kernel, 4).unwrap();
    writer.write_all(b"1234").unwrap();
    assert!(writer.write_all(b"5").is_err());
    writer.finish().unwrap();
    let mut back = Vec::new();
    control
        .open_reader(Slot::B, Role::Kernel)
        .unwrap()
        .read_to_end(&mut back)
        .unwrap();
    assert_eq!(back, b"1234");
    // A shorter payload later shrinks the area: no stale tail is read back.
    let mut writer = control.open_writer(Slot::B, Role::Kernel, 2).unwrap();
    writer.write_all(b"ab").unwrap();
    writer.finish().unwrap();
    assert_eq!(
        fs::read(control.image_path(Slot::B, Role::Kernel)).unwrap(),
        b"ab"
    );
}

#[test]
fn one_damaged_table_copy_is_survived_at_every_write() {
    let (dir, mut control) = control();
    fill(&mut control, Slot::B, 3);
    for round in 0..6u8 {
        // Damage the copy that the next write will not touch... and the one it will:
        // whichever copy is torn, the other one must still give a valid table.
        let before = control.table().unwrap();
        let victim = usize::from(round % 2 == 0);
        let path = dir.path().join(format!("slots.{victim}"));
        let saved = fs::read(&path).unwrap();
        fs::write(&path, &saved[..17]).unwrap();
        match control.table() {
            Ok(table) => assert!(table.seq + 1 >= before.seq),
            Err(e) => panic!("round {round}: {e}"),
        }
        fs::write(&path, saved).unwrap();
        if round % 2 == 0 {
            control
                .mark_bootable(Slot::B, &SlotMeta::default())
                .unwrap();
        } else {
            control.mark_unbootable(Slot::B).unwrap();
        }
    }
    fs::write(dir.path().join("slots.0"), [0u8; 64]).unwrap();
    fs::write(dir.path().join("slots.1"), b"short").unwrap();
    assert!(matches!(control.table(), Err(UpdateError::BootControl)));
    assert!(matches!(
        control.simulate_boot(),
        Err(UpdateError::BootControl)
    ));
}

#[test]
fn torn_write_keeps_the_previous_table() {
    let (dir, mut control) = control();
    fill(&mut control, Slot::B, 3);
    control
        .mark_bootable(Slot::B, &SlotMeta::default())
        .unwrap();
    let before = control.table().unwrap();
    // The next write goes to the copy that does not hold `before`; simulate it being torn.
    let stale = (0..2)
        .find(|i| control.read_copy(*i).unwrap() != Some(before))
        .unwrap();
    fs::write(dir.path().join(format!("slots.{stale}")), [0xAAu8; 40]).unwrap();
    assert_eq!(control.table().unwrap(), before);
    control.set_next_boot_once(Slot::B, 3).unwrap();
    assert_eq!(control.table().unwrap().trial, Some((Slot::B, 3)));
    assert_eq!(control.table().unwrap().seq, before.seq + 1);
}

#[test]
fn no_bootable_slot_is_reported() {
    let (dir, control) = control();
    let mut table = control.table().unwrap();
    table.flags = [SlotFlags::default(); 2];
    table.seq += 1;
    fs::write(dir.path().join("slots.0"), table.encode()).unwrap();
    assert!(matches!(
        control.simulate_boot(),
        Err(UpdateError::NoBootableSlot)
    ));
}

#[test]
fn unchanged_state_is_not_rewritten() {
    let (_dir, mut control) = control();
    let seq = control.table().unwrap().seq;
    control.mark_unbootable(Slot::B).unwrap();
    control.mark_successful().unwrap();
    assert_eq!(control.table().unwrap().seq, seq);
}
