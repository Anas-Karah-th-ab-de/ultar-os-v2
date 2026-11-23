//! The normal life of an update: check, download, apply, restart, confirm – and the ways back.

mod common;

use common::*;
use ultar_update::bootctl::{BootControl, Slot};
use ultar_update::commit::BootOutcome;
use ultar_update::engine::EnginePhase;
use ultar_update::error::UpdateError;
use ultar_update::history::HistoryKind;
use ultar_update::manifest::Role;
use ultar_update::stage::NoPacing;
use ultar_update::version::Version;

fn kinds(
    engine: &ultar_update::engine::Engine<ultar_update::filectl::FileBootControl>,
) -> Vec<HistoryKind> {
    engine.history().iter().map(|e| e.kind).collect()
}

type TestEngine = ultar_update::engine::Engine<ultar_update::filectl::FileBootControl>;

/// Step 1 of the cycle: the factory state, then check and download. Nothing touches a slot.
fn cycle_check_and_download(device: &Device, engine: &mut TestEngine, new: &Release) {
    let status = engine.status().unwrap();
    assert_eq!(status.current_version, Version::new(0, 7, 0));
    assert_eq!(
        (status.phase, status.active_slot),
        (EnginePhase::Idle, Slot::A)
    );
    assert_eq!(status.rollback_floor, 1);
    assert!(!status.previous_startable);

    let mut source = MemSource::new(&new.bundle);
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    assert_eq!(offer.bundle_size(), new.bundle.len() as u64);
    assert_eq!(offer.verified.manifest(), &new.manifest);
    assert!(device.slot_b_untouched(), "checking writes nothing");

    let mut seen = Vec::new();
    engine
        .download(&mut source, &offer, &mut NoPacing, &mut |done, total| {
            seen.push((done, total))
        })
        .unwrap();
    assert_eq!(
        seen.last(),
        Some(&(new.bundle.len() as u64, new.bundle.len() as u64))
    );
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0));
    let status = engine.status().unwrap();
    assert_eq!(
        (status.phase, status.pending_version),
        (EnginePhase::Staged, Some(new.manifest.version))
    );
    assert!(device.slot_b_untouched(), "downloading writes no slot");
}

/// Step 2: install into slot B; the old system keeps running until the restart.
fn cycle_install(device: &Device, engine: &mut TestEngine, new: &Release) {
    let applied = engine.apply(&mut |_, _, _| {}).unwrap();
    assert_eq!(
        (applied.slot, applied.version),
        (Slot::B, new.manifest.version)
    );
    assert_eq!(device.images(Slot::B), new.payloads);
    assert_eq!(
        device.images(Slot::A),
        device.factory,
        "the running slot is never written"
    );
    assert_eq!(device.boot.verity_root(Slot::B).unwrap(), Some([0xAB; 32]));
    let status = engine.status().unwrap();
    assert_eq!(status.phase, EnginePhase::RebootRequired);
    assert_eq!(status.current_version, Version::new(0, 7, 0));
    assert_eq!(
        status.rollback_floor, 1,
        "floor is not raised by installing"
    );
    assert_eq!(
        engine.resume_after_boot(&Health(true)).unwrap(),
        BootOutcome::RebootPending
    );
    assert!(matches!(
        engine.apply(&mut |_, _, _| {}),
        Err(UpdateError::Busy)
    ));
}

/// Step 3: after the restart the new system is confirmed and the old one retired.
fn cycle_confirm(device: &Device, engine: &mut TestEngine, new: &Release) {
    assert_eq!(engine.status().unwrap().phase, EnginePhase::TrialRunning);
    assert_eq!(
        engine.resume_after_boot(&Health(true)).unwrap(),
        BootOutcome::Committed {
            version: Some(new.manifest.version)
        }
    );
    let status = engine.status().unwrap();
    assert_eq!(status.current_version, new.manifest.version);
    assert_eq!(
        (status.phase, status.rollback_floor),
        (EnginePhase::Idle, 2)
    );
    assert_eq!(
        kinds(engine),
        [
            HistoryKind::Staged,
            HistoryKind::Applied,
            HistoryKind::Installed
        ]
    );
    // The factory slot (index 1) is below the new floor (2): it can no longer start.
    assert!(!device.boot.slot_state(Slot::A).unwrap().bootable);
    assert!(!status.previous_startable);
    assert!(matches!(
        engine.boot_previous(),
        Err(UpdateError::PreviousUnavailable)
    ));
    assert_eq!(
        engine.resume_after_boot(&Health(false)).unwrap(),
        BootOutcome::Nothing
    );
}

#[test]
fn full_update_cycle() {
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    let mut engine = device.engine();
    cycle_check_and_download(&device, &mut engine, &new);
    cycle_install(&device, &mut engine, &new);
    drop(engine);

    assert_eq!(device.reboot(&[&new]), (Slot::B, 1));
    let mut engine = device.engine();
    cycle_confirm(&device, &mut engine, &new);
    assert_eq!(device.reboot(&[&new]), (Slot::B, 1));
    // The same release offered again is not an update.
    assert!(engine
        .check(&mut MemSource::new(&new.bundle), "stable")
        .unwrap()
        .is_none());
}

#[test]
fn previous_version_can_be_started_while_not_below_the_floor() {
    let device = Device::new();
    let new = release((0, 7, 1), 1); // same rollback index as the factory system
    let mut engine = device.engine();
    install(&mut engine, &new).unwrap();
    device.reboot(&[&new]);
    let mut engine = device.engine();
    engine.resume_after_boot(&Health(true)).unwrap();
    let status = engine.status().unwrap();
    assert!(status.previous_startable);
    assert_eq!(status.previous_version, Some(Version::new(0, 7, 0)));
    assert_eq!(engine.boot_previous().unwrap(), Some(Version::new(0, 7, 0)));
    assert_eq!(device.reboot(&[&new]), (Slot::A, 0));
    let mut engine = device.engine();
    assert_eq!(
        engine.resume_after_boot(&Health(true)).unwrap(),
        BootOutcome::Nothing
    );
    let status = engine.status().unwrap();
    assert_eq!(status.current_version, Version::new(0, 7, 0));
    assert_eq!(status.previous_version, Some(new.manifest.version));
    assert!(status.previous_startable, "and forward again");
    assert_eq!(
        engine.history().last().unwrap().kind,
        HistoryKind::PreviousSelected
    );
}

#[test]
fn automatic_fallback_after_failed_starts() {
    let device = Device::new();
    let new = release((0, 7, 1), 5);
    let mut engine = device.engine();
    install(&mut engine, &new).unwrap();
    drop(engine);
    // The new system never comes up: three attempts, then the old slot.
    for attempt in 0..3 {
        assert_eq!(device.reboot(&[&new]), (Slot::B, 1), "attempt {attempt}");
    }
    assert_eq!(device.reboot(&[&new]), (Slot::A, 0));
    let mut engine = device.engine();
    assert_eq!(
        engine.resume_after_boot(&Health(true)).unwrap(),
        BootOutcome::RolledBack
    );
    let status = engine.status().unwrap();
    assert_eq!(status.current_version, Version::new(0, 7, 0));
    assert_eq!(
        status.rollback_floor, 1,
        "a release that never ran does not raise the floor"
    );
    assert_eq!(
        status.phase,
        EnginePhase::Staged,
        "the verified bundle is still there"
    );
    let last = engine.history().last().unwrap().clone();
    assert_eq!(last.kind, HistoryKind::RolledBack);
    assert_eq!(last.detail_key.as_deref(), Some("update.error.boot_failed"));
    assert_eq!(
        engine.resume_after_boot(&Health(true)).unwrap(),
        BootOutcome::Nothing
    );
    // It can be tried again.
    engine.apply(&mut |_, _, _| {}).unwrap();
    assert_eq!(device.reboot(&[&new]), (Slot::B, 1));
}

#[test]
fn failed_health_check_gives_the_new_slot_up() {
    let device = Device::new();
    let new = release((0, 7, 1), 5);
    install(&mut device.engine(), &new).unwrap();
    assert_eq!(device.reboot(&[&new]), (Slot::B, 1));
    let mut engine = device.engine();
    assert_eq!(
        engine.resume_after_boot(&Health(false)).unwrap(),
        BootOutcome::HealthFailed
    );
    assert_eq!(engine.status().unwrap().rollback_floor, 1);
    assert!(!device.boot.slot_state(Slot::B).unwrap().bootable);
    assert_eq!(
        device.reboot(&[&new]),
        (Slot::A, 0),
        "next start is the old system at once"
    );
    let mut engine = device.engine();
    assert_eq!(
        engine.resume_after_boot(&Health(true)).unwrap(),
        BootOutcome::Nothing
    );
    assert_eq!(
        engine.history().last().unwrap().detail_key.as_deref(),
        Some("update.error.unhealthy")
    );
}

#[test]
fn cancel_before_restart_disarms_the_new_slot() {
    let device = Device::new();
    let new = release((0, 7, 1), 5);
    let mut engine = device.engine();
    install(&mut engine, &new).unwrap();
    engine.cancel_pending().unwrap();
    let status = engine.status().unwrap();
    assert_eq!(status.phase, EnginePhase::Idle);
    assert_eq!(device.reboot(&[&new]), (Slot::A, 0));
    assert_eq!(
        engine.history().last().unwrap().kind,
        HistoryKind::Cancelled
    );
    assert!(matches!(
        engine.apply(&mut |_, _, _| {}),
        Err(UpdateError::NotStaged)
    ));
}

#[test]
fn second_update_goes_back_into_slot_a() {
    let device = Device::new();
    let first = release((0, 7, 1), 1);
    let second = release((0, 8, 0), 3);
    install(&mut device.engine(), &first).unwrap();
    device.reboot(&[&first]);
    let mut engine = device.engine();
    engine.resume_after_boot(&Health(true)).unwrap();
    install(&mut engine, &second).unwrap();
    assert_eq!(device.images(Slot::A), second.payloads);
    assert_eq!(device.reboot(&[&first, &second]), (Slot::A, 2));
    let mut engine = device.engine();
    assert_eq!(
        engine.resume_after_boot(&Health(true)).unwrap(),
        BootOutcome::Committed {
            version: Some(second.manifest.version)
        }
    );
    assert_eq!(engine.status().unwrap().rollback_floor, 3);
    assert!(
        !device.boot.slot_state(Slot::B).unwrap().bootable,
        "0.7.1 (index 1) is retired"
    );
    // An update that skips a required intermediate version is refused.
    let far = release_with((0, 9, 0), 3, &PUBLISHER, |m| {
        m.min_version = Version::new(0, 8, 5)
    });
    assert!(matches!(
        engine.check(&mut MemSource::new(&far.bundle), "stable"),
        Err(UpdateError::NeedsIntermediate)
    ));
}

#[test]
fn apply_progress_covers_write_and_read_back() {
    let device = Device::new();
    let new = release((0, 7, 1), 1);
    let mut engine = device.engine();
    let mut source = MemSource::new(&new.bundle);
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    engine
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .unwrap();
    let mut events = Vec::new();
    engine
        .apply(&mut |stage, done, total| events.push((stage, done, total)))
        .unwrap();
    let total = 2 * (KERNEL_LEN + SYSTEM_LEN) as u64;
    assert_eq!(events.last().unwrap().1, total);
    assert!(events.iter().all(|e| e.2 == total));
    use ultar_update::apply::ApplyStage::{ReadBack, Write};
    for stage in [
        Write(Role::Kernel),
        Write(Role::System),
        ReadBack(Role::Kernel),
        ReadBack(Role::System),
    ] {
        assert!(events.iter().any(|e| e.0 == stage), "{stage:?}");
    }
}

#[test]
fn free_space_is_checked_with_reserve() {
    let mut device = Device::new();
    let new = release((0, 7, 1), 1);
    device.free_space = new.bundle.len() as u64 + device.config.reserve_bytes - 1;
    let mut engine = device.engine();
    let mut source = MemSource::new(&new.bundle);
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    match engine.download(&mut source, &offer, &mut NoPacing, &mut |_, _| {}) {
        Err(UpdateError::NoSpace { needed, free }) => {
            assert_eq!(needed, free + 1);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(engine.status().unwrap().phase, EnginePhase::Idle);
    device.free_space += 1;
    let mut engine = device.engine();
    engine
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .unwrap();
}

#[test]
fn nothing_offered_and_policy_storage() {
    let device = Device::new();
    let mut engine = device.engine();
    let mut source = MemSource::new(b"");
    source.empty = true;
    assert!(engine.check(&mut source, "stable").unwrap().is_none());
    let mut policy = engine.policy();
    assert!(policy.automatic);
    policy.channel = "beta".into();
    policy.automatic = false;
    engine.set_policy(&policy).unwrap();
    assert_eq!(device.engine().policy(), policy);
}
