//! Anti-rollback, replay and key rotation, seen from the whole engine.

mod common;

use common::*;
use ultar_update::bootctl::Slot;
use ultar_update::commit::BootOutcome;
use ultar_update::error::UpdateError;
use ultar_update::history::HistoryKind;
use ultar_update::keys::{KeySet, Rotation};
use ultar_update::sign::public_of;

fn update_to(device: &Device, release: &Release, known: &[&Release]) {
    install(&mut device.engine(), release).unwrap();
    device.reboot(known);
    let outcome = device.engine().resume_after_boot(&Health(true)).unwrap();
    assert!(
        matches!(outcome, BootOutcome::Committed { .. }),
        "{outcome:?}"
    );
}

fn check(device: &Device, release: &Release) -> Result<bool, UpdateError> {
    device
        .engine()
        .check(&mut MemSource::new(&release.bundle), "stable")
        .map(|offer| offer.is_some())
}

#[test]
fn replay_of_an_old_signed_release_is_refused() {
    let device = Device::new();
    let old = release((0, 7, 1), 2);
    let fixed = release((0, 7, 2), 3); // closes a hole: index raised
    update_to(&device, &old, &[&old]);
    update_to(&device, &fixed, &[&old, &fixed]);
    // A server (or a stick) presents the old, validly signed release again.
    assert!(matches!(
        check(&device, &old),
        Err(UpdateError::RollbackRefused {
            floor: 3,
            offered: 2
        })
    ));
    let mut engine = device.engine();
    let stick = device.write_bundle("old.uup", &old);
    assert!(
        matches!(
            engine.stage_offline(&stick, true, &mut |_, _| {}),
            Err(UpdateError::RollbackRefused { .. })
        ),
        "not even an administrator's downgrade permission goes below the floor"
    );
    // A higher version number does not help an old index either.
    let disguised = release((9, 9, 9), 2);
    assert!(matches!(
        check(&device, &disguised),
        Err(UpdateError::RollbackRefused { .. })
    ));
}

#[test]
fn same_index_older_version_is_refused_online() {
    let device = Device::new();
    let v1 = release((0, 7, 1), 1);
    let v2 = release((0, 7, 2), 1);
    update_to(&device, &v1, &[&v1]);
    update_to(&device, &v2, &[&v1, &v2]);
    assert!(matches!(
        check(&device, &v1),
        Err(UpdateError::DowngradeRefused)
    ));
    assert!(
        !check(&device, &v2).unwrap(),
        "the running version is not an update"
    );
}

#[test]
fn rollback_floor_never_decreases_with_one_store_file_damaged() {
    let device = Device::new();
    let r5 = release((0, 7, 1), 5);
    update_to(&device, &r5, &[&r5]);
    let state = device.config.state_dir.join("state");
    let low = release((0, 7, 2), 4);
    for victim in ["rollback.a", "rollback.b"] {
        let path = state.join(victim);
        let saved = std::fs::read(&path).unwrap();
        for damage in [&b""[..], b"0 0000\n", b"4", &saved[..saved.len() - 2]] {
            std::fs::write(&path, damage).unwrap();
            assert_eq!(
                device.engine().status().unwrap().rollback_floor,
                5,
                "{victim} {damage:?}"
            );
            assert!(matches!(
                check(&device, &low),
                Err(UpdateError::RollbackRefused { floor: 5, .. })
            ));
        }
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            device.engine().status().unwrap().rollback_floor,
            5,
            "{victim} missing"
        );
        std::fs::write(&path, saved).unwrap();
    }
    // An old copy of one file put back (as if restored from a backup) does not lower it.
    let old_value = {
        let other = Device::new();
        let r2 = release((0, 7, 1), 2);
        update_to(&other, &r2, &[&r2]);
        std::fs::read(other.config.state_dir.join("state/rollback.a")).unwrap()
    };
    std::fs::write(state.join("rollback.a"), old_value).unwrap();
    assert_eq!(device.engine().status().unwrap().rollback_floor, 5);
    // The next commit repairs the damaged copy.
    let r6 = release((0, 7, 3), 6);
    update_to(&device, &r6, &[&r5, &r6]);
    let a = std::fs::read(state.join("rollback.a")).unwrap();
    assert_eq!(a, std::fs::read(state.join("rollback.b")).unwrap());
    assert!(a.starts_with(b"6 "));
}

#[test]
fn both_store_files_damaged_fails_closed() {
    let device = Device::new();
    let r5 = release((0, 7, 1), 5);
    update_to(&device, &r5, &[&r5]);
    let state = device.config.state_dir.join("state");
    std::fs::write(state.join("rollback.a"), b"x").unwrap();
    std::fs::write(state.join("rollback.b"), b"y").unwrap();
    let newer = release((0, 7, 2), 9);
    assert!(matches!(
        check(&device, &newer),
        Err(UpdateError::StateCorrupt)
    ));
    assert!(matches!(
        device.engine().status(),
        Err(UpdateError::StateCorrupt)
    ));
    // The device itself keeps starting.
    assert_eq!(device.reboot(&[&r5]), (Slot::B, 1));
}

use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use ultar_update::bootctl::{BootControl, SlotMeta, SlotState, SlotWrite};
use ultar_update::filectl::FileBootControl;
use ultar_update::manifest::Role;

/// File slots plus a firmware-held index (stands for an EFI variable or TPM counter).
struct WithFirmware(FileBootControl, Arc<AtomicU64>);
impl BootControl for WithFirmware {
    fn active_slot(&self) -> Result<Slot, UpdateError> {
        self.0.active_slot()
    }
    fn slot_state(&self, slot: Slot) -> Result<SlotState, UpdateError> {
        self.0.slot_state(slot)
    }
    fn roles(&self) -> Vec<Role> {
        self.0.roles()
    }
    fn mark_unbootable(&mut self, slot: Slot) -> Result<(), UpdateError> {
        self.0.mark_unbootable(slot)
    }
    fn open_writer(
        &mut self,
        slot: Slot,
        role: Role,
        size: u64,
    ) -> Result<Box<dyn SlotWrite>, UpdateError> {
        self.0.open_writer(slot, role, size)
    }
    fn open_reader(&self, slot: Slot, role: Role) -> Result<Box<dyn Read>, UpdateError> {
        self.0.open_reader(slot, role)
    }
    fn mark_bootable(&mut self, slot: Slot, meta: &SlotMeta) -> Result<(), UpdateError> {
        self.0.mark_bootable(slot, meta)
    }
    fn set_next_boot_once(&mut self, slot: Slot, tries: u8) -> Result<(), UpdateError> {
        self.0.set_next_boot_once(slot, tries)
    }
    fn mark_successful(&mut self) -> Result<(), UpdateError> {
        self.0.mark_successful()
    }
    fn set_default(&mut self, slot: Slot) -> Result<(), UpdateError> {
        self.0.set_default(slot)
    }
    fn firmware_rollback_index(&self) -> Result<Option<u64>, UpdateError> {
        Ok(Some(self.1.load(Ordering::SeqCst)))
    }
    fn raise_firmware_rollback_index(&mut self, index: u64) -> Result<(), UpdateError> {
        self.1.fetch_max(index, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn rollback_index_kept_by_the_platform_is_honoured_and_raised() {
    let device = Device::new();
    let firmware = Arc::new(AtomicU64::new(7));
    let open = || {
        let env = ultar_update::engine::Environment {
            clock: device.clock.clone(),
            space: Arc::new(ultar_update::health::FixedSpace(u64::MAX)),
            control: Arc::new(ultar_update::fault::InterruptAfter::never()),
        };
        let boot = WithFirmware(device.boot.clone(), firmware.clone());
        ultar_update::engine::Engine::open(device.config.clone(), env, boot).unwrap()
    };
    // The files say 1, the firmware says 7: the firmware wins (files wiped or replaced).
    let mut engine = open();
    assert_eq!(engine.rollback_floor().unwrap(), 7);
    let r6 = release((0, 7, 1), 6);
    assert!(matches!(
        engine.check(&mut MemSource::new(&r6.bundle), "stable"),
        Err(UpdateError::RollbackRefused {
            floor: 7,
            offered: 6
        })
    ));
    let r9 = release((0, 7, 2), 9);
    let mut source = MemSource::new(&r9.bundle);
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    engine
        .download(
            &mut source,
            &offer,
            &mut ultar_update::stage::NoPacing,
            &mut |_, _| {},
        )
        .unwrap();
    engine.apply(&mut |_, _, _| {}).unwrap();
    assert_eq!(
        firmware.load(Ordering::SeqCst),
        7,
        "not raised before the new system ran"
    );
    device.reboot(&[&r9]);
    let mut engine = open();
    engine.resume_after_boot(&Health(true)).unwrap();
    assert_eq!(firmware.load(Ordering::SeqCst), 9);
}

fn rotation(serial: u64, secrets: &[[u8; 32]], signer: &[u8; 32]) -> Vec<u8> {
    let mut keys: Vec<[u8; 32]> = secrets.iter().map(public_of).collect();
    keys.sort_unstable();
    Rotation {
        set: KeySet { serial, keys },
        created: NOW,
    }
    .sign(signer)
    .unwrap()
}

#[test]
fn key_rotation_travels_in_the_bundle() {
    const NEW_KEY: [u8; 32] = [0x77; 32];
    let device = Device::new();
    // A release signed with a key the device does not know yet ...
    let by_new_key = release_with((0, 7, 1), 1, &NEW_KEY, |_| {});
    assert!(matches!(
        check(&device, &by_new_key),
        Err(UpdateError::UnknownKey)
    ));
    // ... is accepted when the bundle carries a rotation signed by the old key.
    let rotate = rotation(1, &[NEW_KEY], &PUBLISHER);
    let rotated = assemble(
        by_new_key.manifest.clone(),
        by_new_key.payloads.clone(),
        std::slice::from_ref(&rotate),
        &NEW_KEY,
    );
    assert!(check(&device, &rotated).unwrap());
    let engine = device.engine();
    assert_eq!(engine.status().unwrap().key_serial, 1);
    assert!(engine
        .history()
        .iter()
        .any(|e| e.kind == HistoryKind::KeysRotated));
    // From now on the old key is retired: its releases are refused, also after a restart ...
    let by_old_key = release((0, 7, 2), 1);
    assert!(matches!(
        check(&device, &by_old_key),
        Err(UpdateError::UnknownKey)
    ));
    // ... and it cannot rotate itself back in.
    let back = rotation(2, &[PUBLISHER], &PUBLISHER);
    let sneaky = assemble(
        by_old_key.manifest.clone(),
        by_old_key.payloads.clone(),
        &[back],
        &PUBLISHER,
    );
    assert!(matches!(
        check(&device, &sneaky),
        Err(UpdateError::UnknownKey)
    ));
    // Bundles keep carrying the old rotation; that is harmless.
    let later = release_with((0, 7, 3), 1, &NEW_KEY, |_| {});
    let later = assemble(
        later.manifest.clone(),
        later.payloads.clone(),
        &[rotate],
        &NEW_KEY,
    );
    update_to(&device, &later, &[&later]);
    assert_eq!(device.engine().status().unwrap().key_serial, 1);
}

#[test]
fn rotation_by_a_stranger_changes_nothing() {
    let device = Device::new();
    let forged = rotation(1, &[STRANGER], &STRANGER);
    let evil = release_with((0, 7, 1), 1, &STRANGER, |_| {});
    let evil = assemble(
        evil.manifest.clone(),
        evil.payloads.clone(),
        &[forged],
        &STRANGER,
    );
    assert!(matches!(
        check(&device, &evil),
        Err(UpdateError::UnknownKey)
    ));
    assert_eq!(device.engine().status().unwrap().key_serial, 0);
    assert!(
        check(&device, &release((0, 7, 1), 1)).unwrap(),
        "the real publisher still works"
    );
}

#[test]
fn retiring_a_key_does_not_make_the_device_forget_its_own_slots() {
    const NEW_KEY: [u8; 32] = [0x78; 32];
    let device = Device::new();
    let first = release((0, 7, 1), 1);
    update_to(&device, &first, &[&first]);
    // The rotation arrives with a bundle that is only looked at, not installed.
    let rotate = rotation(1, &[NEW_KEY], &PUBLISHER);
    let next = release_with((0, 7, 2), 1, &NEW_KEY, |_| {});
    let next = assemble(
        next.manifest.clone(),
        next.payloads.clone(),
        &[rotate],
        &NEW_KEY,
    );
    assert!(check(&device, &next).unwrap());
    // The running system was signed with the key that is now retired. The device still knows
    // what it runs – otherwise the version checks would fall back to the factory version.
    let status = device.engine().status().unwrap();
    assert_eq!(status.current_version, first.manifest.version);
    assert!(matches!(
        check(&device, &first),
        Err(UpdateError::UnknownKey)
    ));
    let older = release_with((0, 7, 0), 1, &NEW_KEY, |_| {});
    assert!(matches!(
        check(&device, &older),
        Err(UpdateError::DowngradeRefused)
    ));
    update_to(&device, &next, &[&first, &next]);
    let status = device.engine().status().unwrap();
    assert_eq!(status.previous_version, Some(first.manifest.version));
    assert!(status.previous_startable);
}
