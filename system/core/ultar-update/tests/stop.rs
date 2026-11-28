//! Stopping the service (stop signal of the service runtime): a running operation ends at its
//! next checkpoint like a power loss – nothing fetched or staged is thrown away, and a release
//! under test is not judged.

mod common;

use common::rig::*;
use common::*;
use ultar_update::bootctl::{BootControl, Slot};
use ultar_update::commit::BootOutcome;
use ultar_update::error::UpdateError;
use ultar_update::health::HealthCheck;
use ultar_update::service::Job;

const DOWNLOAD: Job = Job::Download {
    allow_metered: true,
};

#[test]
fn stop_during_a_download_keeps_what_was_fetched() {
    let rig = rig();
    rig.publish(&release((0, 7, 1), 2));
    rig.cancel.stop();
    let stopped = rig.job(ADMIN, DOWNLOAD);
    assert!(
        matches!(stopped, Err(UpdateError::Interrupted)),
        "{stopped:?}"
    );
    // The partial download stays for the next start (a cancel would have dropped it).
    let part = rig.device.config.state_dir.join("staging/bundle.part.000");
    assert!(part.exists());
    assert!(
        rig.cancel.is_stopping(),
        "a stop is not reset by the next job"
    );
}

#[test]
fn stop_during_an_installation_keeps_the_staged_bundle_and_the_running_system() {
    let rig = rig();
    let new = release((0, 7, 1), 2);
    rig.publish(&new);
    rig.job(ADMIN, DOWNLOAD).unwrap();
    rig.cancel.stop();
    assert!(matches!(
        rig.job(ADMIN, Job::Apply),
        Err(UpdateError::Interrupted)
    ));
    assert!(!rig.device.boot.slot_state(Slot::B).unwrap().bootable);
    assert_eq!(rig.device.reboot(&[&new]), (Slot::A, 0));
    // Next start of the service: the bundle is still staged and installs.
    let engine = rig.device.engine();
    assert!(engine.staged().unwrap().is_some());
    drop(engine);
    install(&mut rig.device.engine(), &new).unwrap();
    assert_eq!(rig.device.reboot(&[&new]), (Slot::B, 1));
}

/// What the daemon's health wait reports when the stop comes while it waits.
struct StoppedWhileWaiting;

impl HealthCheck for StoppedWhileWaiting {
    fn check(&self) -> Result<(), UpdateError> {
        Err(UpdateError::Interrupted)
    }
}

#[test]
fn stop_while_the_new_release_is_judged_is_no_verdict() {
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    install(&mut device.engine(), &new).unwrap();
    assert_eq!(device.reboot(&[&new]), (Slot::B, 1));
    let judged = device.engine().resume_after_boot(&StoppedWhileWaiting);
    assert!(matches!(judged, Err(UpdateError::Interrupted)));
    let state = device.boot.slot_state(Slot::B).unwrap();
    assert!(
        state.bootable && !state.successful,
        "given up or confirmed without a check"
    );
    assert_eq!(device.engine().rejected_digest(), None);
    // The next start of the service judges it.
    let outcome = device.engine().resume_after_boot(&Health(true)).unwrap();
    assert!(matches!(outcome, BootOutcome::Committed { .. }));
}
