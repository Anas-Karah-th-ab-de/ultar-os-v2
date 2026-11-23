//! Second review round: the policy layer – rights of unprivileged callers, the automatic run
//! and its timer, the stale cancel request, and the shipped bus policy.

mod common;

use common::rig::*;
use common::*;
use ultar_update::bootctl::Slot;
use ultar_update::commit::BootOutcome;
use ultar_update::contract::Phase;
use ultar_update::error::UpdateError;
use ultar_update::service::{Job, UNPRIVILEGED_JOBS, UNPRIVILEGED_WINDOW};

const HOUR: u64 = 3600;
const DOWNLOAD: Job = Job::Download {
    allow_metered: true,
};

#[test]
fn unprivileged_callers_cannot_keep_the_operation_slot_busy() {
    let rig = rig();
    for _ in 0..UNPRIVILEGED_JOBS {
        rig.job(USER, Job::Check).unwrap();
    }
    for _ in 0..3 {
        assert!(matches!(
            rig.core.accept(USER, &Job::Check),
            Err(UpdateError::RateLimited)
        ));
        assert!(matches!(
            rig.core.accept("app.other", &DOWNLOAD),
            Err(UpdateError::RateLimited)
        ));
    }
    // The administrator and the device's own run are never limited by what apps do.
    rig.job(ADMIN, Job::Check).unwrap();
    rig.core.network(NET, false).unwrap();
    rig.publish(&release((0, 7, 1), 2));
    rig.core.tick().unwrap();
    assert_eq!(
        rig.core.status().unwrap().progress.phase,
        Phase::RebootRequired
    );
    // After the window the callers may ask again; a clock set back locks nobody out.
    rig.device.clock.set(NOW + UNPRIVILEGED_WINDOW);
    rig.job(USER, Job::Check).unwrap();
    rig.device.clock.set(NOW - 5 * HOUR);
    rig.job(USER, Job::Check).unwrap();
}

#[test]
fn refused_requests_do_not_use_up_the_allowance() {
    let rig = rig();
    rig.core.accept(ADMIN, &Job::Check).unwrap(); // the slot is taken
    for _ in 0..50 {
        assert!(matches!(
            rig.core.accept(USER, &Job::Check),
            Err(UpdateError::Busy)
        ));
        assert!(matches!(
            rig.core.accept(USER, &Job::Apply),
            Err(UpdateError::PermissionDenied)
        ));
    }
    rig.core.release();
    rig.job(USER, Job::Check).unwrap();
}

#[test]
fn cancel_request_that_came_too_late_does_not_hit_the_next_job() {
    let rig = rig();
    let new = release((0, 7, 1), 2);
    rig.publish(&new);
    rig.job(ADMIN, DOWNLOAD).unwrap();
    // The race: "cancel" saw a running job, the job ended, then the flag was set.
    rig.cancel.cancel();
    rig.job(ADMIN, Job::Apply).unwrap();
    assert_eq!(rig.device.images(Slot::B), new.payloads);
    assert_eq!(
        rig.core.status().unwrap().progress.phase,
        Phase::RebootRequired,
        "the stale request cancelled the next job and dropped what was pending"
    );
}

#[test]
fn administrator_can_still_install_the_release_the_device_gave_up() {
    let new = release((0, 7, 1), 2);
    let rig = rig();
    rig.publish(&new);
    rig.core.network(NET, false).unwrap();
    rig.core.tick().unwrap();
    assert_eq!(rig.device.reboot(&[&new]), (Slot::B, 1));
    let outcome = rig.core.resume_after_boot(&Health(false)).unwrap();
    assert_eq!(outcome, BootOutcome::HealthFailed);
    assert_eq!(rig.device.reboot(&[&new]), (Slot::A, 0));
    rig.core.resume_after_boot(&Health(true)).unwrap();
    // By itself: no. Not from the server, and not from a copy the administrator staged.
    rig.core.tick().unwrap();
    assert_eq!(rig.core.status().unwrap().progress.phase, Phase::Idle);
    rig.job(ADMIN, DOWNLOAD).unwrap();
    rig.core.tick().unwrap();
    assert_eq!(rig.core.status().unwrap().progress.phase, Phase::Staged);
    assert!(!rig.device.boot.table().unwrap().flags[1].bootable);
    // By the administrator's hand: yes. A healthy start then clears the record.
    rig.job(ADMIN, Job::Apply).unwrap();
    assert_eq!(rig.device.reboot(&[&new]), (Slot::B, 1));
    let outcome = rig.core.resume_after_boot(&Health(true)).unwrap();
    assert!(matches!(outcome, BootOutcome::Committed { .. }));
}

#[test]
fn another_release_is_installed_automatically_after_one_was_given_up() {
    let bad = release((0, 7, 1), 1);
    let fixed = release((0, 7, 2), 1);
    let rig = rig();
    rig.publish(&bad);
    rig.core.network(NET, false).unwrap();
    rig.core.tick().unwrap();
    rig.device.reboot(&[&bad]);
    rig.core.resume_after_boot(&Health(false)).unwrap();
    assert_eq!(rig.device.reboot(&[&bad]), (Slot::A, 0));
    rig.core.resume_after_boot(&Health(true)).unwrap();
    rig.core.tick().unwrap();
    assert_eq!(rig.core.status().unwrap().progress.phase, Phase::Idle);

    rig.publish(&fixed);
    rig.core.tick().unwrap();
    let progress = rig.core.status().unwrap().progress;
    assert_eq!(progress.phase, Phase::RebootRequired);
    assert_eq!(progress.version.as_deref(), Some("0.7.2"));
    assert_eq!(rig.device.reboot(&[&bad, &fixed]), (Slot::B, 2));
    rig.core.resume_after_boot(&Health(true)).unwrap();
    assert_eq!(rig.core.status().unwrap().current_version, "0.7.2");
}

#[test]
fn automatic_run_is_due_by_stored_time_not_by_uptime() {
    let rig = rig();
    rig.core.network(NET, false).unwrap();
    assert!(rig.core.automatic_due(6 * HOUR), "never checked: due");
    rig.core.tick_if_due(6 * HOUR).unwrap();
    assert!(!rig.core.automatic_due(6 * HOUR));
    // A "restart" of the daemon two hours later: the stored time still counts.
    rig.device.clock.set(NOW + 2 * HOUR);
    let sources = std::sync::Arc::new(Published(rig.published.clone()));
    let (core, _) = core_on(
        &rig.device,
        sources,
        std::sync::Arc::new(Collect::default()),
    );
    core.network(NET, false).unwrap();
    rig.publish(&release((0, 7, 1), 2));
    core.tick_if_due(6 * HOUR).unwrap();
    assert_eq!(
        core.status().unwrap().progress.phase,
        Phase::Idle,
        "not due"
    );
    // Short uptimes add up: after six hours of wall time the next start checks.
    rig.device.clock.set(NOW + 6 * HOUR);
    let sources = std::sync::Arc::new(Published(rig.published.clone()));
    let (core, _) = core_on(
        &rig.device,
        sources,
        std::sync::Arc::new(Collect::default()),
    );
    core.network(NET, false).unwrap();
    core.tick_if_due(6 * HOUR).unwrap();
    assert_eq!(core.status().unwrap().progress.phase, Phase::RebootRequired);
}

#[test]
fn failed_run_is_retried_soon_and_a_blocked_run_at_every_wake_up() {
    let rig = rig();
    // Blocked by policy (network unknown): nothing is scheduled, it stays due.
    rig.core.tick_if_due(6 * HOUR).unwrap();
    assert!(rig.core.automatic_due(6 * HOUR));
    // A failing run (release for another channel): next try after 15 minutes, not 6 hours.
    rig.core.network(NET, false).unwrap();
    let beta = release_with((0, 7, 1), 2, &PUBLISHER, |m| m.channel = "beta".into());
    rig.publish(&beta);
    assert!(rig.core.tick_if_due(6 * HOUR).is_err());
    assert!(!rig.core.automatic_due(6 * HOUR));
    rig.device.clock.set(NOW + 15 * 60);
    assert!(rig.core.automatic_due(6 * HOUR));
    // A clock that was set back (or a damaged record far in the future) does not postpone
    // the check for ever.
    rig.publish(&release((0, 7, 1), 2));
    rig.core.tick_if_due(6 * HOUR).unwrap();
    rig.device.clock.set(NOW - 30 * 24 * HOUR);
    assert!(rig.core.automatic_due(6 * HOUR));
    let record = rig.device.config.state_dir.join("next_check");
    std::fs::write(&record, b"not a number").unwrap();
    assert!(rig.core.automatic_due(6 * HOUR));
}

#[test]
fn policy_change_waits_for_a_running_operation_and_keeps_offline_bundles() {
    let rig = rig();
    let new = release((0, 7, 1), 2);
    let path = rig.device.write_bundle("new.uup", &new);
    let offline = Job::Offline {
        path,
        allow_downgrade: false,
    };
    rig.job(ADMIN, offline).unwrap();
    let mut policy = rig.core.policy();
    policy.channel = "beta".into();
    rig.core.set_policy(ADMIN, &policy).unwrap();
    // A bundle the administrator brought on a stick belongs to no channel: it stays.
    assert_eq!(rig.core.status().unwrap().progress.phase, Phase::Staged);
    rig.job(ADMIN, Job::Apply).unwrap();
    assert_eq!(rig.device.images(Slot::B), new.payloads);
}

#[test]
fn shipped_bus_policy_protects_the_update_service() {
    use ultar_bus::{Identity, Pattern, Policy};
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../rootfs/etc/ultar/bus-policy.toml"
    );
    let policy = Policy::load(std::path::Path::new(path)).unwrap();
    let id = |name: &str| Identity::new(name).unwrap();
    let progress = Pattern::parse("update.progress").unwrap();
    let service = id("system.updated");
    assert!(policy.may_register(&service, "update"));
    assert!(policy.may_publish(&service, "update.progress"));
    for other in [
        "app.files",
        "app.store",
        "system.net",
        "system.admin",
        "system.devd",
    ] {
        let who = id(other);
        assert!(
            !policy.may_publish(&who, "update.progress"),
            "{other} can fake progress"
        );
        assert!(
            !policy.may_register(&who, "update"),
            "{other} can pose as the service"
        );
    }
    let app = id("app.files");
    assert!(policy.may_subscribe(&app, &progress));
    for method in ["status", "check", "download", "history", "get_policy"] {
        assert!(policy.may_call(&app, "update", method), "{method}");
    }
    for method in [
        "apply",
        "cancel",
        "offline",
        "boot_previous",
        "set_policy",
        "network",
    ] {
        assert!(!policy.may_call(&app, "update", method), "{method}");
    }
    assert!(policy.may_call(&id("system.net"), "update", "network"));
    assert!(!policy.may_call(&id("system.net"), "update", "apply"));
    for method in ultar_update::contract::METHODS {
        assert!(
            policy.may_call(&id("system.admin"), "update", method),
            "{method}"
        );
    }
}
