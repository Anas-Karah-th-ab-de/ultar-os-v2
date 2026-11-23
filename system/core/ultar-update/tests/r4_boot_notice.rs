//! "The new release did not start" stays visible in the status until something newer happens –
//! also after the daemon's first automatic run and after a daemon restart – and says the same as
//! the history.

mod common;

use common::rig::*;
use common::*;
use std::sync::{Arc, Mutex};
use ultar_update::bootctl::Slot;
use ultar_update::history::HistoryKind;
use ultar_update::service::Job;

const BOOT_FAILED: Option<&str> = Some("update.error.boot_failed");

fn rolled_back_rig() -> (Rig, Release) {
    let rig = rig();
    let new = release((0, 7, 1), 1);
    rig.publish(&new);
    rig.job(
        ADMIN,
        Job::Download {
            allow_metered: true,
        },
    )
    .unwrap();
    rig.job(ADMIN, Job::Apply).unwrap();
    for _ in 0..3 {
        rig.device.reboot(&[&new]);
    }
    assert_eq!(rig.device.reboot(&[&new]), (Slot::A, 0));
    rig.core.resume_after_boot(&Health(true)).unwrap();
    (rig, new)
}

fn error_key(rig: &Rig) -> Option<String> {
    rig.core.status().unwrap().progress.error_key
}

#[test]
fn notice_survives_the_first_automatic_run_and_a_daemon_restart() {
    let (rig, _new) = rolled_back_rig();
    assert_eq!(error_key(&rig).as_deref(), BOOT_FAILED);
    // The timer's first run right after the start: it checks, finds only the given-up release
    // and ends without news.
    rig.core.tick().unwrap();
    assert_eq!(
        error_key(&rig).as_deref(),
        BOOT_FAILED,
        "status after the automatic run"
    );
    let history = rig.core.history(1);
    assert_eq!(
        history.last().map(|e| e.kind_key.as_str()),
        Some(HistoryKind::RolledBack.key())
    );
    // A daemon restart without a device restart: the start-up check has nothing to judge.
    let published = Arc::new(Mutex::new(None));
    let (core, _) = core_on(
        &rig.device,
        Arc::new(Published(published)),
        Arc::new(Collect::default()),
    );
    assert_eq!(
        core.status().unwrap().progress.error_key.as_deref(),
        BOOT_FAILED
    );
}

#[test]
fn notice_ends_with_the_next_history_entry() {
    let (rig, _new) = rolled_back_rig();
    // An administrator installs the release by hand: downloading is newer news.
    rig.job(
        ADMIN,
        Job::Download {
            allow_metered: true,
        },
    )
    .unwrap();
    assert_eq!(error_key(&rig), None);
    assert_eq!(
        rig.core.history(1).last().map(|e| e.kind_key.as_str()),
        Some(HistoryKind::Staged.key())
    );
}
