//! Every way a release can be wrong – and that nothing reaches a slot in any of them.

mod common;

use common::*;
use ultar_update::engine::EnginePhase;
use ultar_update::error::{Malformed, UpdateError};
use ultar_update::manifest::Role;
use ultar_update::stage::NoPacing;
use ultar_update::version::Version;

/// Runs check → download → apply against `bundle` and returns the first error.
fn attempt(device: &Device, bundle: &[u8]) -> UpdateError {
    let mut engine = device.engine();
    let mut source = MemSource::new(bundle);
    let result = (|| {
        let offer = engine
            .check(&mut source, "stable")?
            .ok_or(UpdateError::NoUpdate)?;
        engine.download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})?;
        engine.apply(&mut |_, _, _| {}).map(|_| ())
    })();
    let error = result.expect_err("a bad release was installed");
    assert!(
        device.slot_b_untouched(),
        "{error}: something was written to the slot"
    );
    assert_eq!(device.reboot(&[]), (ultar_update::bootctl::Slot::A, 0));
    error
}

#[test]
fn bad_signature() {
    let device = Device::new();
    let good = release((0, 7, 1), 1);
    // Flip one bit in the signed manifest text (inside the header).
    let mut bundle = good.bundle.clone();
    bundle[8 + 6 + 30] ^= 0x01;
    assert!(matches!(
        attempt(&device, &bundle),
        UpdateError::BadSignature
    ));
    // Flip one bit in the signature itself.
    let mut bundle = good.bundle.clone();
    let last_sig_byte = good.header.raw.len() - 1;
    bundle[last_sig_byte] ^= 0x80;
    assert!(matches!(
        attempt(&device, &bundle),
        UpdateError::BadSignature
    ));
}

#[test]
fn unknown_key_and_no_keys() {
    let device = Device::new();
    let foreign = release_with((0, 7, 1), 1, &STRANGER, |_| {});
    assert!(matches!(
        attempt(&device, &foreign.bundle),
        UpdateError::UnknownKey
    ));
    std::fs::remove_dir_all(&device.config.keys_dir).unwrap();
    let good = release((0, 7, 1), 1);
    assert!(matches!(
        attempt(&device, &good.bundle),
        UpdateError::NoTrustedKeys
    ));
}

#[test]
fn app_package_key_directory_is_not_consulted() {
    // The update keys are their own directory: a key that only exists elsewhere is unknown.
    let device = Device::new();
    let other_dir = device.dir.path().join("trusted-keys");
    std::fs::create_dir_all(&other_dir).unwrap();
    std::fs::write(
        other_dir.join("apps.ed25519"),
        hex::encode(ultar_update::sign::public_of(&STRANGER)),
    )
    .unwrap();
    let foreign = release_with((0, 7, 1), 1, &STRANGER, |_| {});
    assert!(matches!(
        attempt(&device, &foreign.bundle),
        UpdateError::UnknownKey
    ));
}

#[test]
fn expired_and_clock_behaviour() {
    let device = Device::new();
    let old = release_with((0, 7, 1), 1, &PUBLISHER, |m| {
        m.created = NOW - 90 * 86_400;
        m.expires = NOW - 86_400 - 1; // one second past the tolerance
    });
    assert!(matches!(
        attempt(&device, &old.bundle),
        UpdateError::Expired
    ));
    let edge = release_with((0, 7, 1), 1, &PUBLISHER, |m| {
        m.created = NOW - 90 * 86_400;
        m.expires = NOW - 86_400;
    });
    let mut engine = device.engine();
    assert!(engine
        .check(&mut MemSource::new(&edge.bundle), "stable")
        .unwrap()
        .is_some());
}

#[test]
fn device_without_a_clock_uses_the_stored_time_floor() {
    let device = Device::new();
    device.clock.set(0); // no battery-backed clock: 1970
    let recent = release((0, 7, 1), 1);
    let stale = release_with((0, 7, 2), 1, &PUBLISHER, |m| {
        m.created = NOW - 400 * 86_400;
        m.expires = NOW - 300 * 86_400;
    });
    let mut engine = device.engine();
    // With no evidence of the time the stale release cannot be told apart yet ...
    assert!(engine
        .check(&mut MemSource::new(&stale.bundle), "stable")
        .unwrap()
        .is_some());
    // ... but once a newer signed manifest has been seen, time never goes back before it.
    assert!(engine
        .check(&mut MemSource::new(&recent.bundle), "stable")
        .unwrap()
        .is_some());
    assert!(matches!(
        engine.check(&mut MemSource::new(&stale.bundle), "stable"),
        Err(UpdateError::Expired)
    ));
    drop(engine);
    let mut engine = device.engine();
    assert!(
        matches!(
            engine.check(&mut MemSource::new(&stale.bundle), "stable"),
            Err(UpdateError::Expired)
        ),
        "the floor survives a restart"
    );
    // A clock set far into the future does not poison the stored floor.
    device.clock.set(NOW + 10_000 * 86_400);
    assert!(matches!(
        engine.check(&mut MemSource::new(&recent.bundle), "stable"),
        Err(UpdateError::Expired)
    ));
    device.clock.set(0);
    assert!(engine
        .check(&mut MemSource::new(&recent.bundle), "stable")
        .unwrap()
        .is_some());
}

#[test]
fn wrong_edition_arch_device_channel() {
    use ultar_update::manifest::Manifest;
    type Edit = fn(&mut Manifest);
    type Expect = fn(&UpdateError) -> bool;
    let device = Device::new();
    let cases: [(Edit, Expect); 4] = [
        (
            |m| m.edition = "phone".into(),
            |e| matches!(e, UpdateError::WrongEdition),
        ),
        (
            |m| m.arch = "aarch64".into(),
            |e| matches!(e, UpdateError::WrongArch),
        ),
        (
            |m| m.devices = vec!["a30".into(), "op15".into()],
            |e| matches!(e, UpdateError::WrongDevice),
        ),
        (
            |m| m.channel = "beta".into(),
            |e| matches!(e, UpdateError::WrongChannel),
        ),
    ];
    for (edit, expected) in cases {
        let bad = release_with((0, 7, 1), 1, &PUBLISHER, edit);
        let error = attempt(&device, &bad.bundle);
        assert!(expected(&error), "{error}");
    }
}

#[test]
fn lower_rollback_index_and_old_versions() {
    let mut device = Device::new();
    device.config.factory_rollback_index = 4;
    let low = release((0, 7, 1), 3);
    assert!(matches!(
        attempt(&device, &low.bundle),
        UpdateError::RollbackRefused {
            floor: 4,
            offered: 3
        }
    ));
    let older = release_with((0, 6, 9), 4, &PUBLISHER, |m| {
        m.min_version = Version::new(0, 6, 0)
    });
    assert!(matches!(
        attempt(&device, &older.bundle),
        UpdateError::DowngradeRefused
    ));
    let same = release((0, 7, 0), 4);
    assert!(matches!(
        attempt(&device, &same.bundle),
        UpdateError::NoUpdate
    ));
}

#[test]
fn payload_set_must_match_the_slots() {
    let device = Device::new();
    let kernel_only = {
        let full = release((0, 7, 1), 1);
        let mut manifest = full.manifest.clone();
        manifest.payloads.truncate(1);
        assemble(manifest, vec![full.payloads[0].clone()], &[], &PUBLISHER)
    };
    assert!(matches!(
        attempt(&device, &kernel_only.bundle),
        UpdateError::PayloadSetMismatch
    ));
}

#[test]
fn hash_mismatch_in_each_payload() {
    let good = release((0, 7, 1), 1);
    let header = good.header.raw.len();
    for (role, offset) in [
        (Role::Kernel, header),
        (Role::Kernel, header + KERNEL_LEN - 1),
        (Role::System, header + KERNEL_LEN),
        (Role::System, good.bundle.len() - 1),
    ] {
        let device = Device::new();
        let mut bundle = good.bundle.clone();
        bundle[offset] ^= 0x40;
        match attempt(&device, &bundle) {
            UpdateError::HashMismatch(found) => assert_eq!(found, role),
            other => panic!("{other}"),
        }
        let engine = device.engine();
        assert_eq!(
            engine.status().unwrap().phase,
            EnginePhase::Idle,
            "bad bytes are not kept"
        );
    }
}

#[test]
fn truncated_download_is_kept_for_resume_but_never_applied() {
    let device = Device::new();
    let good = release((0, 7, 1), 1);
    for cut in [
        0,
        1,
        KERNEL_LEN,
        good.bundle.len() - good.header.raw.len() - 1,
    ] {
        let mut bundle = good.bundle.clone();
        bundle.truncate(good.header.raw.len() + cut);
        assert!(
            matches!(attempt(&device, &bundle), UpdateError::Truncated),
            "cut {cut}"
        );
        let mut engine = device.engine();
        assert!(matches!(
            engine.apply(&mut |_, _, _| {}),
            Err(UpdateError::NotStaged)
        ));
    }
    // A header cut short is not even a bundle.
    let mut bundle = good.bundle.clone();
    bundle.truncate(good.header.raw.len() - 10);
    assert!(matches!(
        attempt(&device, &bundle),
        UpdateError::Malformed(Malformed::Length)
    ));
}

#[test]
fn more_bytes_than_signed() {
    let device = Device::new();
    let good = release((0, 7, 1), 1);
    let mut bundle = good.bundle.clone();
    bundle.push(0);
    assert!(matches!(
        attempt(&device, &bundle),
        UpdateError::SizeExceeded
    ));
    assert_eq!(device.engine().status().unwrap().phase, EnginePhase::Idle);
    // A manifest that signs a smaller size than the payload really has: the stream is cut at
    // the signed size and the hash (over the signed size) decides.
    let lying = {
        let mut manifest = good.manifest.clone();
        manifest.payloads[1].size -= 10;
        assemble(manifest, good.payloads.clone(), &[], &PUBLISHER)
    };
    assert!(matches!(
        attempt(&device, &lying.bundle),
        UpdateError::HashMismatch(Role::System)
    ));
}

#[test]
fn staged_bundle_modified_after_verification_is_caught_at_apply() {
    let device = Device::new();
    let good = release((0, 7, 1), 1);
    let mut engine = device.engine();
    let mut source = MemSource::new(&good.bundle);
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    engine
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .unwrap();
    let staged = device.config.state_dir.join("staging/bundle.uup.000");
    let mut bytes = std::fs::read(&staged).unwrap();
    let at = bytes.len() - 5;
    bytes[at] ^= 1;
    std::fs::write(&staged, bytes).unwrap();
    assert!(matches!(
        engine.apply(&mut |_, _, _| {}),
        Err(UpdateError::HashMismatch(Role::System))
    ));
    assert!(
        !ultar_update::bootctl::BootControl::slot_state(
            &device.boot,
            ultar_update::bootctl::Slot::B
        )
        .unwrap()
        .bootable
    );
    assert_eq!(device.reboot(&[&good]), (ultar_update::bootctl::Slot::A, 0));
    let engine = device.engine();
    assert_eq!(engine.status().unwrap().phase, EnginePhase::Idle);
    assert_eq!(
        engine.history().last().unwrap().detail_key.as_deref(),
        Some("update.error.hash_mismatch")
    );
}

#[test]
fn staged_header_swapped_for_another_signed_release_is_refused() {
    // The marker binds the staged file to one manifest digest.
    let device = Device::new();
    let good = release((0, 7, 1), 1);
    let other = release((0, 7, 2), 1);
    let mut engine = device.engine();
    let mut source = MemSource::new(&good.bundle);
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    engine
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .unwrap();
    std::fs::write(
        device.config.state_dir.join("staging/bundle.uup.000"),
        &other.bundle,
    )
    .unwrap();
    assert!(matches!(
        engine.apply(&mut |_, _, _| {}),
        Err(UpdateError::NotStaged)
    ));
    assert!(device.slot_b_untouched());
}

use std::io::Read;
use ultar_update::bootctl::{BootControl, Slot, SlotMeta, SlotState, SlotWrite};
use ultar_update::filectl::FileBootControl;

/// A medium that silently damages what is written to the system area.
struct BadMedium(FileBootControl);
impl BootControl for BadMedium {
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
        let mut bytes = Vec::new();
        self.0.open_reader(slot, role)?.read_to_end(&mut bytes)?;
        if role == Role::System {
            bytes[77] ^= 0xFF;
        }
        Ok(Box::new(std::io::Cursor::new(bytes)))
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
}

#[test]
fn slot_that_does_not_keep_its_bytes_fails_read_back() {
    let device = Device::new();
    let good = release((0, 7, 1), 1);
    let env = ultar_update::engine::Environment {
        clock: device.clock.clone(),
        space: std::sync::Arc::new(ultar_update::health::FixedSpace(u64::MAX)),
        control: std::sync::Arc::new(ultar_update::fault::InterruptAfter::never()),
    };
    let mut engine = ultar_update::engine::Engine::open(
        device.config.clone(),
        env,
        BadMedium(device.boot.clone()),
    )
    .unwrap();
    let mut source = MemSource::new(&good.bundle);
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    engine
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .unwrap();
    assert!(matches!(
        engine.apply(&mut |_, _, _| {}),
        Err(UpdateError::ReadBackMismatch(Role::System))
    ));
    assert!(!device.boot.slot_state(Slot::B).unwrap().bootable);
    assert_eq!(device.reboot(&[&good]), (Slot::A, 0));
}
