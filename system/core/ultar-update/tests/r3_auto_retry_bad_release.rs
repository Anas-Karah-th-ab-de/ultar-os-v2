//! r3 finding F1 (major): automatic mode installs a release again that this device has just
//! rejected (failed health check, failed starts) or that the administrator has just left with
//! "start previous version". Nothing remembers the rejected release, so every second start of
//! the device goes into the broken system again.
//!
//! Expected: after a release was given up, the automatic path does not arm the same signed
//! manifest again (an administrator may still apply it by hand).

mod common;

use common::rig::*;
use common::*;
use std::sync::Arc;
use ultar_update::bootctl::{BootControl, Slot};
use ultar_update::commit::BootOutcome;
use ultar_update::contract::Phase;
use ultar_update::filectl::FileBootControl;
use ultar_update::service::Core;

/// What the daemon does after a start: a fresh service core on the same device.
fn daemon_start(rig: &Rig) -> Arc<Core<FileBootControl>> {
    let sources = Arc::new(Published(rig.published.clone()));
    let (core, _) = core_on(&rig.device, sources, Arc::new(Collect::default()));
    core.network(NET, false).unwrap();
    core
}

/// A device on which the automatic path has installed `new`; the restart is pending.
fn installed_automatically(new: &Release) -> Rig {
    let rig = rig();
    rig.publish(new);
    rig.core.network(NET, false).unwrap();
    rig.core.tick().unwrap();
    let phase = rig.core.status().unwrap().progress.phase;
    assert_eq!(phase, Phase::RebootRequired);
    rig
}

fn assert_not_armed_again(rig: &Rig, core: &Core<FileBootControl>) {
    core.tick().unwrap();
    let progress = core.status().unwrap().progress;
    let slot_b = rig.device.boot.slot_state(Slot::B).unwrap();
    assert!(
        progress.phase != Phase::RebootRequired && slot_b.trial_tries.is_none(),
        "the rejected release is armed again: {progress:?}"
    );
}

#[test]
fn release_that_failed_its_health_check_is_not_installed_again_automatically() {
    let new = release((0, 7, 1), 2);
    let rig = installed_automatically(&new);
    assert_eq!(rig.device.reboot(&[&new]), (Slot::B, 1));
    let core = daemon_start(&rig);
    let outcome = core.resume_after_boot(&Health(false)).unwrap();
    assert_eq!(outcome, BootOutcome::HealthFailed);
    assert_eq!(rig.device.reboot(&[&new]), (Slot::A, 0));
    let core = daemon_start(&rig);
    core.resume_after_boot(&Health(true)).unwrap();
    assert_not_armed_again(&rig, &core);
}

#[test]
fn release_that_never_started_is_not_installed_again_automatically() {
    let new = release((0, 7, 1), 2);
    let rig = installed_automatically(&new);
    for _ in 0..3 {
        assert_eq!(rig.device.reboot(&[&new]).0, Slot::B, "a failed start");
    }
    assert_eq!(rig.device.reboot(&[&new]), (Slot::A, 0), "fallback");
    let core = daemon_start(&rig);
    let outcome = core.resume_after_boot(&Health(true)).unwrap();
    assert_eq!(outcome, BootOutcome::RolledBack);
    assert_not_armed_again(&rig, &core);
}

#[test]
fn previous_version_chosen_by_the_administrator_is_not_undone_automatically() {
    // Same rollback index, so the old system stays startable after the update.
    let new = release((0, 7, 1), 1);
    let rig = installed_automatically(&new);
    assert_eq!(rig.device.reboot(&[&new]), (Slot::B, 1));
    let core = daemon_start(&rig);
    let outcome = core.resume_after_boot(&Health(true)).unwrap();
    assert!(matches!(outcome, BootOutcome::Committed { .. }));
    core.boot_previous(ADMIN).unwrap();
    assert_eq!(rig.device.reboot(&[&new]), (Slot::A, 0));
    let core = daemon_start(&rig);
    core.resume_after_boot(&Health(true)).unwrap();
    assert_not_armed_again(&rig, &core);
}
