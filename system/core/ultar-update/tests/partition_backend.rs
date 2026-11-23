//! The partition backend (`partctl`) through the whole engine: install into the other slot's
//! partitions, the loader's start decision on `ULTAR-BOOTCTL`, confirm, fall back, power loss.
//!
//! Partitions are plain files named by their GPT name, exactly as the Ultar kernel offers them
//! under `partition_dir`. "Restart" = `PartitionBootControl::loader_start` (the loader's two
//! shared functions) + a fresh engine over a backend that runs the chosen slot.

mod common;

use common::*;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use ultar_slots::layout::{self, BOOTCTL_PART, META_OFFSETS, TABLE_COPY_OFFSETS};
use ultar_update::bootctl::{BootControl, Slot, SlotMeta};
use ultar_update::commit::BootOutcome;
use ultar_update::engine::{Engine, Environment};
use ultar_update::error::UpdateError;
use ultar_update::fault::{Checkpoint, InterruptAfter};
use ultar_update::health::FixedSpace;
use ultar_update::manifest::Role;
use ultar_update::partctl::{read_running_slot, PartitionBootControl};
use ultar_update::slottable::SlotTable;
use ultar_update::stage::NoPacing;
use ultar_update::version::Version;

const KERNEL_PART: u64 = 256 * 1024;
const SYSTEM_PART: u64 = 512 * 1024;

struct PartDevice {
    base: Device,
    parts: PathBuf,
}

fn make_part(dir: &Path, name: &str, size: u64) {
    let f = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(dir.join(name))
        .unwrap();
    f.set_len(size).unwrap();
}

impl PartDevice {
    fn new() -> Self {
        let base = Device::new();
        let parts = base.dir.path().join("parts");
        fs::create_dir_all(&parts).unwrap();
        make_part(&parts, BOOTCTL_PART, layout::BOOTCTL_INSTALL_BYTES);
        for slot in Slot::BOTH {
            make_part(&parts, layout::kernel_part(slot), KERNEL_PART);
            make_part(&parts, layout::system_part(slot), SYSTEM_PART);
        }
        PartitionBootControl::format(&parts).unwrap();
        let dev = Self { base, parts };
        for (role, bytes) in ROLES.iter().zip(&dev.base.factory) {
            fs::OpenOptions::new()
                .write(true)
                .open(dev.node(*role, Slot::A))
                .unwrap()
                .write_all_at(bytes, 0)
                .unwrap();
        }
        dev
    }

    fn node(&self, role: Role, slot: Slot) -> PathBuf {
        self.parts.join(match role {
            Role::Kernel => layout::kernel_part(slot),
            _ => layout::system_part(slot),
        })
    }

    fn control(&self, running: Slot) -> PartitionBootControl {
        PartitionBootControl::open(&self.parts, running, &ROLES).unwrap()
    }

    fn engine(&self, running: Slot, control: Arc<dyn Checkpoint>) -> Engine<PartitionBootControl> {
        let env = Environment {
            clock: self.base.clock.clone(),
            space: Arc::new(FixedSpace(u64::MAX)),
            control,
        };
        Engine::open(self.base.config.clone(), env, self.control(running)).unwrap()
    }

    /// The first `len` bytes of each role's partition of `slot`.
    fn images(&self, slot: Slot, lens: &[usize]) -> Vec<Vec<u8>> {
        ROLES
            .iter()
            .zip(lens)
            .map(|(role, len)| {
                let mut buf = vec![0u8; *len];
                fs::File::open(self.node(*role, slot))
                    .unwrap()
                    .read_exact_at(&mut buf, 0)
                    .unwrap();
                buf
            })
            .collect()
    }

    /// The loader's step; then the started slot must hold exactly one complete known system,
    /// and so must every other slot the loader could start.
    fn reboot(&self, known: &[&Release]) -> (Slot, usize) {
        let slot = PartitionBootControl::loader_start(&self.parts)
            .expect("a slot must always be startable");
        let mut candidates = vec![&self.base.factory];
        candidates.extend(known.iter().map(|r| &r.payloads));
        let system_in = |slot: Slot| {
            candidates
                .iter()
                .position(|c| self.images(slot, &c.iter().map(Vec::len).collect::<Vec<_>>()) == **c)
                .unwrap_or_else(|| panic!("slot {slot:?} holds an incomplete or unknown system"))
        };
        let control = self.control(slot);
        if control.slot_state(slot.other()).unwrap().bootable {
            system_in(slot.other());
        }
        (slot, system_in(slot))
    }
}

fn apply(engine: &mut Engine<PartitionBootControl>, new: &Release) -> Result<(), UpdateError> {
    let mut source = MemSource::new(&new.bundle);
    let offer = engine
        .check(&mut source, "stable")?
        .ok_or(UpdateError::NoUpdate)?;
    engine.download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})?;
    engine.apply(&mut |_, _, _| {})?;
    Ok(())
}

#[test]
fn full_cycle_install_trial_confirm() {
    let dev = PartDevice::new();
    let new = release((0, 8, 0), 2);
    assert_eq!(dev.reboot(&[&new]), (Slot::A, 0));
    let mut engine = dev.engine(Slot::A, Arc::new(InterruptAfter::never()));
    apply(&mut engine, &new).unwrap();
    let lens: Vec<usize> = new.payloads.iter().map(Vec::len).collect();
    assert_eq!(dev.images(Slot::B, &lens), new.payloads);
    assert_eq!(
        dev.images(Slot::A, &[5000, 9000]),
        dev.base.factory,
        "running slot untouched"
    );
    assert_eq!(
        dev.control(Slot::A).verity_root(Slot::B).unwrap(),
        Some([0xAB; 32])
    );
    assert_eq!(
        dev.control(Slot::A).table().unwrap().trial,
        Some((Slot::B, 3))
    );
    drop(engine);
    assert_eq!(dev.reboot(&[&new]), (Slot::B, 1));
    assert_eq!(
        dev.control(Slot::B).table().unwrap().trial,
        Some((Slot::B, 2))
    );
    let mut engine = dev.engine(Slot::B, Arc::new(InterruptAfter::never()));
    let outcome = engine.resume_after_boot(&Health(true)).unwrap();
    assert_eq!(
        outcome,
        BootOutcome::Committed {
            version: Some(Version::new(0, 8, 0))
        }
    );
    let table = dev.control(Slot::B).table().unwrap();
    assert_eq!((table.default, table.trial), (Slot::B, None));
    assert!(table.slot(Slot::B).successful);
    drop(engine);
    for _ in 0..3 {
        assert_eq!(dev.reboot(&[&new]), (Slot::B, 1));
    }
}

#[test]
fn a_release_that_never_confirms_falls_back_after_the_tries() {
    let dev = PartDevice::new();
    let new = release((0, 8, 0), 2);
    let mut engine = dev.engine(Slot::A, Arc::new(InterruptAfter::never()));
    apply(&mut engine, &new).unwrap();
    drop(engine);
    for left in [2u8, 1, 0] {
        assert_eq!(dev.reboot(&[&new]), (Slot::B, 1));
        assert_eq!(
            dev.control(Slot::B).table().unwrap().trial,
            Some((Slot::B, left))
        );
    }
    assert_eq!(dev.reboot(&[&new]), (Slot::A, 0), "automatic fallback");
    assert!(!dev.control(Slot::A).slot_state(Slot::B).unwrap().bootable);
    let mut engine = dev.engine(Slot::A, Arc::new(InterruptAfter::never()));
    assert_eq!(
        engine.resume_after_boot(&Health(true)).unwrap(),
        BootOutcome::RolledBack
    );
    assert_eq!(engine.current_version().unwrap(), Version::new(0, 7, 0));
}

#[test]
fn failed_health_check_gives_the_release_up() {
    let dev = PartDevice::new();
    let new = release((0, 8, 0), 2);
    apply(
        &mut dev.engine(Slot::A, Arc::new(InterruptAfter::never())),
        &new,
    )
    .unwrap();
    assert_eq!(dev.reboot(&[&new]), (Slot::B, 1));
    let mut engine = dev.engine(Slot::B, Arc::new(InterruptAfter::never()));
    assert_eq!(
        engine.resume_after_boot(&Health(false)).unwrap(),
        BootOutcome::HealthFailed
    );
    drop(engine);
    assert_eq!(dev.reboot(&[&new]), (Slot::A, 0));
}

/// Power loss at every checkpoint of apply and commit, then restarts until done.
#[test]
fn power_loss_at_every_checkpoint_always_starts_a_complete_system() {
    let new = release((0, 8, 0), 2);
    let mut completed_without_fault = false;
    for n in 0..200u64 {
        let dev = PartDevice::new();
        let fault = Arc::new(InterruptAfter::new(n));
        let first = apply(&mut dev.engine(Slot::A, fault.clone()), &new);
        let mut slot = Slot::A;
        if first.is_ok() {
            let (s, which) = dev.reboot(&[&new]);
            assert_eq!((s, which), (Slot::B, 1));
            slot = s;
            let second = dev
                .engine(slot, fault.clone())
                .resume_after_boot(&Health(true));
            if second.is_ok() {
                completed_without_fault = true;
            }
        }
        // carry on with restarts and no faults until the release is confirmed
        let mut done = false;
        for _ in 0..6 {
            let (s, _) = dev.reboot(&[&new]);
            slot = s;
            let mut engine = dev.engine(slot, Arc::new(InterruptAfter::never()));
            let outcome = engine.resume_after_boot(&Health(true)).unwrap();
            if outcome == BootOutcome::Nothing
                && engine.current_version().unwrap() != new.manifest.version
            {
                apply(&mut engine, &new).unwrap();
            }
            if engine.current_version().unwrap() == new.manifest.version
                && dev.control(slot).slot_state(slot).unwrap().successful
            {
                done = true;
                break;
            }
        }
        assert!(done, "checkpoint {n}: the update never completed");
        assert_eq!(slot, Slot::B);
        if completed_without_fault {
            return;
        }
    }
    panic!("the sweep never ran through");
}

#[test]
fn torn_table_write_leaves_the_current_copy() {
    let dev = PartDevice::new();
    let new = release((0, 8, 0), 2);
    apply(
        &mut dev.engine(Slot::A, Arc::new(InterruptAfter::never())),
        &new,
    )
    .unwrap();
    let control = dev.control(Slot::A);
    let current = control.table().unwrap();
    // The loader's next write goes to the copy that does not hold `current`: tear it.
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(dev.parts.join(BOOTCTL_PART))
        .unwrap();
    let mut copies = [[0u8; 64]; 2];
    for (c, o) in copies.iter_mut().zip(TABLE_COPY_OFFSETS) {
        file.read_exact_at(c, o).unwrap();
    }
    let stale = usize::from(SlotTable::decode(&copies[0]).ok() == Some(current));
    let mut torn = copies[1 - stale];
    torn[20..].fill(0x5A);
    file.write_all_at(&torn, TABLE_COPY_OFFSETS[stale]).unwrap();
    assert_eq!(control.table().unwrap(), current);
    assert_eq!(dev.reboot(&[&new]), (Slot::B, 1), "the trial still starts");
    // Both copies destroyed: no slot, the loader goes to rescue.
    for o in TABLE_COPY_OFFSETS {
        file.write_all_at(&[0u8; 64], o).unwrap();
    }
    assert!(matches!(
        PartitionBootControl::loader_start(&dev.parts),
        Err(UpdateError::BootControl)
    ));
}

#[test]
fn refusals() {
    let dev = PartDevice::new();
    let mut control = dev.control(Slot::A);
    assert!(matches!(
        control.open_writer(Slot::A, Role::System, 10),
        Err(UpdateError::InvalidArgument)
    ));
    assert!(matches!(
        control.mark_bootable(Slot::A, &SlotMeta::default()),
        Err(UpdateError::InvalidArgument)
    ));
    assert!(matches!(
        control.mark_unbootable(Slot::A),
        Err(UpdateError::InvalidArgument)
    ));
    assert!(matches!(
        control.set_next_boot_once(Slot::A, 3),
        Err(UpdateError::InvalidArgument)
    ));
    assert!(matches!(
        control.set_next_boot_once(Slot::B, 3),
        Err(UpdateError::InvalidArgument)
    ));
    assert!(matches!(
        control.set_default(Slot::B),
        Err(UpdateError::PreviousUnavailable)
    ));
    match control.open_writer(Slot::B, Role::Kernel, KERNEL_PART + 1) {
        Err(UpdateError::NoSpace { needed, free }) => {
            assert_eq!((needed, free), (KERNEL_PART + 1, KERNEL_PART))
        }
        _ => panic!("payload larger than the partition must be refused"),
    }
    let mut w = control.open_writer(Slot::B, Role::Kernel, 4).unwrap();
    assert!(std::io::Write::write(&mut w, b"12345").is_err());
    assert!(matches!(
        PartitionBootControl::open(&dev.parts, Slot::A, &[Role::Boot]),
        Err(UpdateError::InvalidArgument)
    ));
    assert!(matches!(
        PartitionBootControl::open(&dev.parts, Slot::A, &[]),
        Err(UpdateError::InvalidArgument)
    ));
    assert!(
        matches!(
            PartitionBootControl::format(&dev.parts),
            Err(UpdateError::InvalidArgument)
        ),
        "never resets a device"
    );
}

#[test]
fn legacy_layouts_and_bad_geometry_are_refused() {
    let dev = PartDevice::new();
    // An installation from before A/B: no ULTAR-BOOTCTL, or a kernel that names no slot.
    let legacy = dev.base.dir.path().join("legacy");
    fs::create_dir_all(&legacy).unwrap();
    assert!(matches!(
        PartitionBootControl::open(&legacy, Slot::A, &ROLES),
        Err(UpdateError::LayoutNotAb)
    ));
    let slot_file = dev.base.dir.path().join("boot_slot");
    for (text, want) in [
        ("a\n", Ok(Slot::A)),
        ("b", Ok(Slot::B)),
        ("-\n", Err("layout")),
        ("x", Err("bc")),
        ("", Err("bc")),
    ] {
        fs::write(&slot_file, text).unwrap();
        match (read_running_slot(&slot_file), want) {
            (Ok(s), Ok(w)) => assert_eq!(s, w),
            (Err(UpdateError::LayoutNotAb), Err("layout"))
            | (Err(UpdateError::BootControl), Err("bc")) => {}
            (got, _) => panic!("{text:?}: {got:?}"),
        }
    }
    assert!(matches!(
        read_running_slot(&legacy.join("none")),
        Err(UpdateError::LayoutNotAb)
    ));
    // Too small a boot control partition.
    let small = dev.base.dir.path().join("small");
    fs::create_dir_all(&small).unwrap();
    make_part(&small, BOOTCTL_PART, layout::BOOTCTL_MIN_BYTES - 512);
    assert!(matches!(
        PartitionBootControl::format(&small),
        Err(UpdateError::BootControl)
    ));
    assert!(matches!(
        PartitionBootControl::open(&small, Slot::A, &ROLES),
        Err(UpdateError::BootControl)
    ));
}

#[test]
fn slot_records_are_checked() {
    let dev = PartDevice::new();
    let mut control = dev.control(Slot::A);
    assert_eq!(control.verity_root(Slot::B).unwrap(), None);
    control
        .mark_bootable(
            Slot::B,
            &SlotMeta {
                verity_root: Some([7; 32]),
            },
        )
        .unwrap();
    assert_eq!(control.verity_root(Slot::B).unwrap(), Some([7; 32]));
    let file = OpenOptions::new()
        .write(true)
        .open(dev.parts.join(BOOTCTL_PART))
        .unwrap();
    file.write_all_at(b"X", META_OFFSETS[1] + 9).unwrap();
    assert!(
        matches!(control.verity_root(Slot::B), Err(UpdateError::BootControl)),
        "damaged record of a bootable slot"
    );
    control.mark_unbootable(Slot::B).unwrap();
    assert_eq!(
        control.verity_root(Slot::B).unwrap(),
        None,
        "unbootable: the record is not trusted, not needed"
    );
}
