//! Power loss at every step: whatever happens, the device starts a complete, verified system.
//!
//! The operation is stopped at checkpoint `n` (every state-changing step, and every 64 KiB of
//! downloading and slot writing) without any clean-up – as a power loss would. Then the device
//! "restarts": the boot code picks a slot, a fresh engine is opened on the same directories and
//! the update is carried on. This is repeated for every `n` until the operation runs through.

mod common;

use common::*;
use std::sync::Arc;
use ultar_update::bootctl::{BootControl, Slot};
use ultar_update::commit::BootOutcome;
use ultar_update::engine::Engine;
use ultar_update::error::UpdateError;
use ultar_update::fault::InterruptAfter;
use ultar_update::filectl::FileBootControl;
use ultar_update::stage::NoPacing;
use ultar_update::version::Version;

/// One complete update attempt on whatever state the device is in. Mirrors what the daemon
/// does after a start: resume, then check/download/apply when nothing is pending.
fn carry_on(
    engine: &mut Engine<FileBootControl>,
    new: &Release,
) -> Result<BootOutcome, UpdateError> {
    let outcome = engine.resume_after_boot(&Health(true))?;
    if outcome != BootOutcome::Nothing {
        return Ok(outcome);
    }
    if engine.current_version()? == new.manifest.version {
        return Ok(BootOutcome::Nothing);
    }
    let mut source = MemSource::new(&new.bundle);
    let Some(offer) = engine.check(&mut source, "stable")? else {
        return Ok(BootOutcome::Nothing);
    };
    engine.download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})?;
    engine.apply(&mut |_, _, _| {})?;
    Ok(BootOutcome::RebootPending)
}

/// Drives the device with unlimited restarts until the new version is confirmed.
fn finish(device: &Device, known: &[&Release], new: &Release) {
    for _ in 0..6 {
        let (_, which) = device.reboot(known);
        let mut engine = device.engine();
        carry_on(&mut engine, new).unwrap();
        let done = which == known.len()
            && engine.current_version().unwrap() == new.manifest.version
            && device
                .boot
                .slot_state(device.boot.active_slot().unwrap())
                .unwrap()
                .successful;
        if done {
            assert_eq!(
                engine.status().unwrap().rollback_floor,
                new.manifest.rollback_index
            );
            return;
        }
    }
    panic!("the update never completed after the interruption");
}

/// Interrupts the update to `new` at checkpoint 0, 1, 2, … on a device made by `prepare`
/// (which returns the device and the releases already on it). Returns how many checkpoints a
/// complete run has.
fn sweep(prepare: &dyn Fn() -> (Device, Vec<Release>), new: &Release) -> u64 {
    for n in 0.. {
        let (device, earlier) = prepare();
        let mut known: Vec<&Release> = earlier.iter().collect();
        known.push(new);
        let new_index = known.len();
        let (old_slot, old_index) = device.reboot(&known);
        let old_floor = device.engine().status().unwrap().rollback_floor;
        let fault = Arc::new(InterruptAfter::new(n));
        // Phase 1: download + apply, interrupted at checkpoint n (if it has that many).
        let first = {
            let mut engine = device.engine_with(fault.clone()).unwrap();
            carry_on(&mut engine, new)
        };
        let mut interrupted = matches!(first, Err(UpdateError::Interrupted));
        if !interrupted {
            first.unwrap();
            // Phase 2: restart into the new slot; the commit is interrupted at what is left.
            assert_eq!(device.reboot(&known), (old_slot.other(), new_index));
            let mut engine = device.engine_with(fault.clone()).unwrap();
            match carry_on(&mut engine, new) {
                Err(UpdateError::Interrupted) => interrupted = true,
                other => {
                    let version = Some(new.manifest.version);
                    assert_eq!(other.unwrap(), BootOutcome::Committed { version });
                }
            }
        }
        // Whatever happened: the device starts a complete system – the old one in its old
        // slot, or the new one in the other slot ...
        let (slot, which) = device.reboot(&known);
        assert!(
            (slot, which) == (old_slot, old_index)
                || (slot, which) == (old_slot.other(), new_index),
            "checkpoint {n}: slot {slot:?} holds system {which}"
        );
        // ... the rollback floor only moved if the new system really ran ...
        let floor = device.engine().status().unwrap().rollback_floor;
        let raised = floor == new.manifest.rollback_index && which == new_index;
        assert!(
            floor == old_floor || raised,
            "checkpoint {n}: floor {floor}"
        );
        // ... and the update can always be completed afterwards.
        {
            let mut engine = device.engine();
            carry_on(&mut engine, new).unwrap();
        }
        finish(&device, &known, new);
        if !interrupted {
            return n;
        }
    }
    unreachable!()
}

#[test]
fn interruption_at_every_checkpoint_first_update() {
    let new = release((0, 7, 1), 2);
    let covered = sweep(&|| (Device::new(), Vec::new()), &new);
    // Sanity: the sweep really walked through chunked transfers and all named steps.
    let chunks = (KERNEL_LEN + SYSTEM_LEN) as u64 / (64 * 1024);
    assert!(
        covered > 2 * chunks + 15,
        "only {covered} checkpoints were exercised"
    );
}

#[test]
fn interruption_at_every_checkpoint_when_the_target_slot_holds_a_working_system() {
    // Second update: slot A still holds the bootable, proven factory system and is now the
    // one being overwritten. It must stop being startable before its first byte changes.
    let first = release((0, 7, 1), 1);
    let new = release((0, 7, 2), 2);
    let prepare = || {
        let device = Device::new();
        install(&mut device.engine(), &first).unwrap();
        device.reboot(&[&first]);
        let outcome = device.engine().resume_after_boot(&Health(true)).unwrap();
        assert!(matches!(outcome, BootOutcome::Committed { .. }));
        assert!(device.boot.slot_state(Slot::A).unwrap().bootable);
        (device, vec![first.clone()])
    };
    let covered = sweep(&prepare, &new);
    assert!(covered > 20);
}

#[test]
fn slot_writes_are_interrupted_inside_every_64_kib_chunk_boundary() {
    // Counts the checkpoints of a clean run to prove the granularity: one per 64 KiB of
    // download and one per 64 KiB of slot writing.
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    let counter = Arc::new(InterruptAfter::never());
    let mut engine = device.engine_with(counter.clone()).unwrap();
    let mut source = MemSource::new(&new.bundle);
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    let before = counter.seen();
    engine
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .unwrap();
    let download = counter.seen() - before;
    let body = (KERNEL_LEN + SYSTEM_LEN) as u64;
    assert_eq!(
        download,
        body.div_ceil(64 * 1024) + 2,
        "chunks + rename + marker"
    );
    let before = counter.seen();
    engine.apply(&mut |_, _, _| {}).unwrap();
    let apply = counter.seen() - before;
    let write_chunks =
        (KERNEL_LEN as u64).div_ceil(64 * 1024) + (SYSTEM_LEN as u64).div_ceil(64 * 1024);
    // journal, unbootable, forget, chunks, 2 flushes, 2 read-backs, manifest, bootable, journal, arm
    assert_eq!(apply, write_chunks + 11);
}

#[test]
fn half_written_slot_is_never_startable() {
    // Stop in the middle of the system image and look at the device before any recovery ran.
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    let mut probe = device.engine();
    let mut source = MemSource::new(&new.bundle);
    let offer = probe.check(&mut source, "stable").unwrap().unwrap();
    probe
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .unwrap();
    drop(probe);
    // 3 steps + 2 kernel chunks + flush + 2 of 4 system chunks.
    let mut engine = device
        .engine_with(Arc::new(InterruptAfter::new(8)))
        .unwrap();
    assert!(matches!(
        engine.apply(&mut |_, _, _| {}),
        Err(UpdateError::Interrupted)
    ));
    drop(engine);
    let half = device.images(Slot::B);
    assert_eq!(half[0], new.payloads[0], "kernel complete");
    assert_ne!(half[1], new.payloads[1], "system incomplete");
    let state = device.boot.slot_state(Slot::B).unwrap();
    assert!(!state.bootable && state.trial_tries.is_none());
    for _ in 0..5 {
        assert_eq!(device.reboot(&[&new]), (Slot::A, 0));
    }
    // After the restart the bundle is still staged and the installation simply runs again.
    let mut engine = device.engine();
    assert_eq!(
        engine.status().unwrap().phase,
        ultar_update::engine::EnginePhase::Staged
    );
    engine.apply(&mut |_, _, _| {}).unwrap();
    assert_eq!(device.reboot(&[&new]), (Slot::B, 1));
}

#[test]
fn power_loss_while_the_slot_table_is_written() {
    // A torn slot table copy at any moment of the cycle never leaves the device without a
    // startable, complete system.
    let new = release((0, 7, 1), 2);
    for victim in 0..2 {
        for stage in 0..3 {
            let device = Device::new();
            let tear = |device: &Device| {
                let path = device.dir.path().join(format!("boot/slots.{victim}"));
                std::fs::write(path, [0x5Au8; 31]).unwrap();
            };
            if stage == 0 {
                tear(&device);
            }
            install(&mut device.engine(), &new).unwrap();
            if stage == 1 {
                tear(&device);
            }
            let (_, which) = device.reboot(&[&new]);
            if stage == 2 {
                tear(&device);
            }
            let mut engine = device.engine();
            carry_on(&mut engine, &new).unwrap();
            let (_, after) = device.reboot(&[&new]);
            assert!(which <= 1 && after <= 1, "victim {victim} stage {stage}");
            assert!(engine.current_version().unwrap() >= Version::new(0, 7, 0));
        }
    }
}

#[test]
fn damaged_journal_does_not_block_or_brick() {
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    install(&mut device.engine(), &new).unwrap();
    std::fs::write(device.config.state_dir.join("journal"), b"\x00\x01garbage").unwrap();
    // The trial is armed in the boot control; the journal is only a hint.
    assert_eq!(device.reboot(&[&new]), (Slot::B, 1));
    let mut engine = device.engine();
    assert_eq!(
        engine.resume_after_boot(&Health(true)).unwrap(),
        BootOutcome::Committed {
            version: Some(new.manifest.version)
        }
    );
    assert_eq!(engine.status().unwrap().rollback_floor, 2);
}
