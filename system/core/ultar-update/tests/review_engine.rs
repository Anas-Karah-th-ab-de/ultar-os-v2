//! Second review round: the engine side – staged bundles that can no longer be installed,
//! private state, time without a clock, and the given-up slot after a daemon restart.

mod common;

use common::*;
use std::sync::Arc;
use ultar_update::bootctl::{BootControl, Slot};
use ultar_update::commit::BootOutcome;
use ultar_update::engine::EnginePhase;
use ultar_update::error::UpdateError;
use ultar_update::fault::InterruptAfter;
use ultar_update::keys::{KeySet, Rotation};
use ultar_update::sign::public_of;
use ultar_update::stage::NoPacing;

fn stage(device: &Device, release: &Release) {
    let mut engine = device.engine();
    let mut source = MemSource::new(&release.bundle);
    let offer = engine.check(&mut source, "stable").unwrap().unwrap();
    engine
        .download(&mut source, &offer, &mut NoPacing, &mut |_, _| {})
        .unwrap();
}

#[test]
fn staged_bundle_that_is_refused_at_install_is_dropped() {
    // Expired meanwhile.
    let device = Device::new();
    stage(&device, &release((0, 7, 1), 2));
    device.clock.set(NOW + 400 * 86_400);
    let mut engine = device.engine();
    assert!(matches!(
        engine.apply(&mut |_, _, _| {}),
        Err(UpdateError::Expired)
    ));
    assert_eq!(engine.status().unwrap().phase, EnginePhase::Idle);
    assert!(device.slot_b_untouched());

    // Signed by a key that a rotation has retired since.
    const NEW_KEY: [u8; 32] = [0x66; 32];
    let device = Device::new();
    stage(&device, &release((0, 7, 1), 2));
    let rotation = Rotation {
        set: KeySet {
            serial: 1,
            keys: vec![public_of(&NEW_KEY)],
        },
        created: NOW,
    }
    .sign(&PUBLISHER)
    .unwrap();
    let next = release_with((0, 7, 2), 2, &NEW_KEY, |_| {});
    let next = assemble(
        next.manifest.clone(),
        next.payloads.clone(),
        &[rotation],
        &NEW_KEY,
    );
    let mut engine = device.engine();
    let looked = engine.check(&mut MemSource::new(&next.bundle), "stable");
    assert!(looked.unwrap().is_some());
    assert!(matches!(
        engine.apply(&mut |_, _, _| {}),
        Err(UpdateError::UnknownKey)
    ));
    assert_eq!(engine.status().unwrap().phase, EnginePhase::Idle);
    assert!(device.slot_b_untouched());
}

#[test]
fn bundle_of_another_channel_is_not_installed_and_goes_with_the_channel() {
    let device = Device::new();
    stage(&device, &release((0, 7, 1), 2));
    let engine = device.engine();
    assert_eq!(
        engine.staged().unwrap().unwrap().channel.as_deref(),
        Some("stable")
    );
    // The policy file is replaced behind the engine's back (as an older daemon would have).
    let mut policy = engine.policy();
    policy.channel = "lts".into();
    let text = "automatic=1\nwindow=-\nchannel=lts\nallow_metered=0\nrate_limit_kib=0\n";
    std::fs::write(device.config.state_dir.join("policy"), text).unwrap();
    let mut engine = device.engine();
    assert!(matches!(
        engine.apply(&mut |_, _, _| {}),
        Err(UpdateError::WrongChannel)
    ));
    assert_eq!(engine.status().unwrap().phase, EnginePhase::Idle);
    // Through the engine the bundle goes at once.
    policy.channel = "stable".into();
    engine.set_policy(&policy).unwrap();
    stage(&device, &release((0, 7, 1), 2));
    policy.channel = "lts".into();
    device.engine().set_policy(&policy).unwrap();
    assert!(device.engine().staged().unwrap().is_none());
}

#[test]
fn same_bundle_from_a_stick_takes_over_the_staged_copy_without_a_second_transfer() {
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    stage(&device, &new);
    let path = device.write_bundle("new.uup", &new);
    let mut engine = device.engine();
    let mut steps = 0;
    engine
        .stage_offline(&path, false, &mut |_, _| steps += 1)
        .unwrap();
    assert_eq!(steps, 0, "nothing was copied again");
    assert_eq!(engine.staged().unwrap().unwrap().channel, None);
}

#[cfg(unix)]
#[test]
fn state_directory_that_others_can_write_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let device = Device::new();
    drop(device.engine());
    let state = &device.config.state_dir;
    let open = |mode: u32| {
        std::fs::set_permissions(state, std::fs::Permissions::from_mode(mode)).unwrap();
        device
            .engine_with(Arc::new(InterruptAfter::never()))
            .map(|_| ())
    };
    for mode in [0o777, 0o770, 0o702] {
        assert!(
            matches!(open(mode), Err(UpdateError::StateCorrupt)),
            "{mode:o}"
        );
    }
    open(0o755).unwrap();
    open(0o700).unwrap();
    // State files themselves are private too.
    install(&mut device.engine(), &release((0, 7, 1), 2)).unwrap();
    for name in ["journal", "state/time.a", "slots/b.manifest", "history"] {
        let mode = std::fs::metadata(state.join(name))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "{name}");
    }
}

#[test]
fn image_build_time_bounds_the_clock_of_a_new_device() {
    // A new device without a set clock (1970) and without any time evidence yet.
    let mut device = Device::new();
    device.clock.set(0);
    let long_expired = release_with((0, 7, 1), 1, &PUBLISHER, |m| {
        m.created = NOW - 900 * 86_400;
        m.expires = NOW - 800 * 86_400;
    });
    let accepted = device
        .engine()
        .check(&mut MemSource::new(&long_expired.bundle), "stable");
    assert!(
        accepted.unwrap().is_some(),
        "without a bound it cannot know"
    );
    // With the build time of its own image as lower bound it does.
    device.config.image_time = NOW - 30 * 86_400;
    let refused = device
        .engine()
        .check(&mut MemSource::new(&long_expired.bundle), "stable");
    assert!(matches!(refused, Err(UpdateError::Expired)));
    let current = release((0, 7, 2), 1);
    let offered = device
        .engine()
        .check(&mut MemSource::new(&current.bundle), "stable");
    assert!(offered.unwrap().is_some());
}

#[test]
fn updater_restart_after_a_failed_health_check_keeps_the_verdict() {
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    install(&mut device.engine(), &new).unwrap();
    assert_eq!(device.reboot(&[&new]), (Slot::B, 1));
    let outcome = device.engine().resume_after_boot(&Health(false)).unwrap();
    assert_eq!(outcome, BootOutcome::HealthFailed);
    // The daemon is restarted, the device is not; the services answer now.
    for _ in 0..3 {
        let outcome = device.engine().resume_after_boot(&Health(true));
        assert_eq!(outcome.unwrap(), BootOutcome::HealthFailed);
    }
    assert!(!device.boot.slot_state(Slot::B).unwrap().bootable);
    assert_eq!(device.engine().status().unwrap().rollback_floor, 1);
    assert_eq!(device.reboot(&[&new]), (Slot::A, 0));
    let outcome = device.engine().resume_after_boot(&Health(true)).unwrap();
    assert_eq!(outcome, BootOutcome::Nothing);
}

#[test]
fn time_floor_learns_only_from_accepted_releases() {
    let device = Device::new();
    device.clock.set(0);
    let mut engine = device.engine();
    for edit in [
        (|m| m.edition = "phone".into()) as fn(&mut ultar_update::manifest::Manifest),
        |m| m.arch = "aarch64".into(),
        |m| m.channel = "beta".into(),
        |m| m.rollback_index = 0,
    ] {
        let foreign = release_with((0, 7, 1), 1, &PUBLISHER, |m| {
            m.created = NOW * 1000;
            m.expires = NOW * 1000 + 1;
            edit(m);
        });
        assert!(engine
            .check(&mut MemSource::new(&foreign.bundle), "stable")
            .is_err());
        assert_eq!(
            engine.now().unwrap(),
            0,
            "a refused manifest moved the time"
        );
    }
    let good = release((0, 7, 1), 1);
    engine
        .check(&mut MemSource::new(&good.bundle), "stable")
        .unwrap();
    assert_eq!(engine.now().unwrap(), good.manifest.created);
}
