//! The update service core: permissions, policy, automatic work, cancel.

mod common;

use common::rig::*;
use common::*;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};
use ultar_update::bootctl::Slot;
use ultar_update::contract::{Phase, PolicyMessage};
use ultar_update::engine::{Engine, Environment};
use ultar_update::error::UpdateError;
use ultar_update::fault::CancelFlag;
use ultar_update::health::FixedSpace;
use ultar_update::service::{Access, Core, Job, SourceFactory};
use ultar_update::source::{Query, Remote, UpdateSource};

const DOWNLOAD: Job = Job::Download {
    allow_metered: true,
};
const DOWNLOAD_UNMETERED_ONLY: Job = Job::Download {
    allow_metered: false,
};

/// Check, download (only with consent on an unknown network) and install (admin only).
fn manual_flow_steps(rig: &Rig) {
    rig.job(USER, Job::Check).unwrap();
    let status = rig.core.status().unwrap();
    assert_eq!(status.progress.phase, Phase::Available);
    assert_eq!(status.progress.version.as_deref(), Some("0.7.1"));
    assert!(status.progress.critical);
    assert_eq!(
        status.progress.notes_key.as_deref(),
        Some("update.notes.test")
    );
    assert_eq!(
        (status.current_version.as_str(), status.slot.as_str()),
        ("0.7.0", "a")
    );
    assert_eq!(status.metered, None);

    // The kind of network is unknown: a download needs the user's explicit consent.
    assert!(matches!(
        rig.job(USER, DOWNLOAD_UNMETERED_ONLY),
        Err(UpdateError::Metered)
    ));
    assert_eq!(
        rig.core.status().unwrap().progress.error_key.as_deref(),
        Some("update.error.metered")
    );
    // ... and that consent is the administrator's (or the policy's), not any caller's.
    assert!(matches!(rig.job(USER, DOWNLOAD), Err(UpdateError::Metered)));
    rig.core.network(NET, false).unwrap();
    rig.job(USER, DOWNLOAD_UNMETERED_ONLY).unwrap();
    assert_eq!(rig.core.status().unwrap().progress.phase, Phase::Staged);

    assert!(matches!(
        rig.job(USER, Job::Apply),
        Err(UpdateError::PermissionDenied)
    ));
    assert!(rig.device.slot_b_untouched());
    rig.job(ADMIN, Job::Apply).unwrap();
    let status = rig.core.status().unwrap();
    assert_eq!(status.progress.phase, Phase::RebootRequired);
    assert_eq!(status.progress.version.as_deref(), Some("0.7.1"));
    assert!(status.progress.critical);
}

#[test]
fn manual_flow_with_progress_events() {
    let rig = rig();
    let new = release_with((0, 7, 1), 2, &PUBLISHER, |m| m.critical = true);
    rig.publish(&new);
    manual_flow_steps(&rig);

    let phases = rig.phases();
    let expected = [
        Phase::Idle,
        Phase::Checking,
        Phase::Available,
        Phase::Checking,
        Phase::Downloading,
        Phase::Staged,
        Phase::Applying,
        Phase::RebootRequired,
    ];
    let mut at = 0;
    for phase in &phases {
        if at < expected.len() && *phase == expected[at] {
            at += 1;
        }
    }
    assert_eq!(at, expected.len(), "{phases:?}");
    // Progress is throttled to percentage steps: bounded number of events.
    assert!(rig.events.0.lock().unwrap().len() < 260);
    let history = rig.core.history(10);
    assert_eq!(history[0].kind_key, "update.history.applied");
    assert_eq!(history[1].kind_key, "update.history.staged");
    assert_eq!(rig.core.history(1).len(), 1);
}

#[test]
fn only_one_operation_at_a_time() {
    let rig = rig();
    rig.core.accept(USER, &Job::Check).unwrap();
    assert!(matches!(
        rig.core.accept(USER, &Job::Check),
        Err(UpdateError::Busy)
    ));
    assert!(matches!(
        rig.core.accept(ADMIN, &Job::Apply),
        Err(UpdateError::Busy)
    ));
    rig.core.tick().unwrap(); // the timer does nothing while an operation is accepted
    rig.core.release();
    rig.core.accept(USER, &Job::Check).unwrap();
    rig.core.run(Job::Check).unwrap();
    rig.core.accept(USER, &Job::Check).unwrap();
}

#[test]
fn admin_only_calls_are_refused_for_everyone_else() {
    let rig = rig();
    let policy = rig.core.policy();
    for caller in [USER, NET, "", "system.settings.evil", "SYSTEM.SETTINGS"] {
        assert!(
            matches!(rig.core.cancel(caller), Err(UpdateError::PermissionDenied)),
            "{caller}"
        );
        assert!(matches!(
            rig.core.boot_previous(caller),
            Err(UpdateError::PermissionDenied)
        ));
        assert!(matches!(
            rig.core.set_policy(caller, &policy),
            Err(UpdateError::PermissionDenied)
        ));
        assert!(matches!(
            rig.core.accept(caller, &Job::Apply),
            Err(UpdateError::PermissionDenied)
        ));
        let offline = Job::Offline {
            path: rig.device.dir.path().join("usb"),
            allow_downgrade: false,
        };
        assert!(matches!(
            rig.core.accept(caller, &offline),
            Err(UpdateError::PermissionDenied)
        ));
    }
    for caller in [USER, ADMIN, ""] {
        assert!(matches!(
            rig.core.network(caller, false),
            Err(UpdateError::PermissionDenied)
        ));
    }
    assert_eq!(rig.core.status().unwrap().metered, None);
}

#[test]
fn with_no_administrators_configured_nobody_is_one() {
    let device = Device::new();
    let cancel = Arc::new(CancelFlag::new());
    let env = Environment {
        clock: device.clock.clone(),
        space: Arc::new(FixedSpace(u64::MAX)),
        control: cancel.clone(),
    };
    let engine = Engine::open(device.config.clone(), env, device.boot.clone()).unwrap();
    let published = Arc::new(Published(Arc::new(Mutex::new(None))));
    let core = Core::new(
        engine,
        cancel,
        published,
        Arc::new(Collect::default()),
        Access::default(),
    );
    assert!(!core.is_admin(ADMIN) && !core.is_admin(""));
    assert!(matches!(
        core.accept(ADMIN, &Job::Apply),
        Err(UpdateError::PermissionDenied)
    ));
    assert!(matches!(
        core.network(NET, false),
        Err(UpdateError::PermissionDenied)
    ));
}

#[test]
fn policy_is_validated_and_stored() {
    let rig = rig();
    let mut policy = rig.core.policy();
    assert!(policy.automatic && !policy.allow_metered);
    policy.channel = "beta".into();
    policy.window = Some((60, 120));
    policy.rate_limit_kib = 100;
    rig.core.set_policy(ADMIN, &policy).unwrap();
    assert_eq!(rig.core.policy(), policy);
    assert_eq!(rig.core.status().unwrap().channel, "beta");
    for bad in [
        PolicyMessage {
            channel: "../../etc".into(),
            ..policy.clone()
        },
        PolicyMessage {
            window: Some((5000, 1)),
            ..policy.clone()
        },
        PolicyMessage {
            rate_limit_kib: u32::MAX,
            ..policy.clone()
        },
    ] {
        assert!(matches!(
            rig.core.set_policy(ADMIN, &bad),
            Err(UpdateError::InvalidArgument)
        ));
    }
    assert_eq!(rig.core.policy(), policy);
    // The channel of the policy is what is asked for – and what the manifest must say.
    rig.publish(&release((0, 7, 1), 2)); // a "stable" release
    assert!(matches!(
        rig.job(USER, Job::Check),
        Err(UpdateError::WrongChannel)
    ));
}

#[test]
fn automatic_updates_follow_policy_network_and_window() {
    let rig = rig();
    let new = release((0, 7, 1), 2);
    rig.publish(&new);
    // Unknown network counts as metered: nothing happens.
    rig.core.tick().unwrap();
    assert!(rig.device.slot_b_untouched());
    assert_eq!(rig.core.status().unwrap().progress.phase, Phase::Idle);
    rig.core.network(NET, true).unwrap();
    rig.core.tick().unwrap();
    assert!(rig.device.slot_b_untouched());
    // Outside the window: nothing.
    rig.core.network(NET, false).unwrap();
    let minute = ((NOW / 60) % 1440) as u16;
    let mut policy = rig.core.policy();
    policy.window = Some(((minute + 60) % 1440, (minute + 120) % 1440));
    rig.core.set_policy(ADMIN, &policy).unwrap();
    rig.core.tick().unwrap();
    assert!(rig.device.slot_b_untouched());
    // Switched off: nothing, even inside the window.
    policy.window = Some((minute, (minute + 1) % 1440));
    policy.automatic = false;
    rig.core.set_policy(ADMIN, &policy).unwrap();
    rig.core.tick().unwrap();
    assert!(rig.device.slot_b_untouched());
    // Allowed: checked, downloaded, installed – and never restarted by itself.
    policy.automatic = true;
    rig.core.set_policy(ADMIN, &policy).unwrap();
    rig.core.tick().unwrap();
    assert_eq!(
        rig.core.status().unwrap().progress.phase,
        Phase::RebootRequired
    );
    assert_eq!(rig.device.images(Slot::B), new.payloads);
    assert_eq!(rig.device.boot.table().unwrap().last_booted, Some(Slot::A));
    rig.core.tick().unwrap(); // nothing more to do while waiting for the restart
    assert_eq!(
        rig.core.status().unwrap().progress.phase,
        Phase::RebootRequired
    );
}

#[test]
fn automatic_update_on_metered_network_only_with_permission() {
    let rig = rig();
    rig.publish(&release((0, 7, 1), 2));
    rig.core.network(NET, true).unwrap();
    let mut policy = rig.core.policy();
    policy.allow_metered = true;
    rig.core.set_policy(ADMIN, &policy).unwrap();
    rig.core.tick().unwrap();
    assert_eq!(
        rig.core.status().unwrap().progress.phase,
        Phase::RebootRequired
    );
}

/// A source whose stream presses "cancel" after some bytes.
struct CancelMidway {
    bundle: Vec<u8>,
    cancel: Arc<CancelFlag>,
}

struct Trigger {
    data: std::io::Cursor<Vec<u8>>,
    left: usize,
    cancel: Arc<CancelFlag>,
}

impl Read for Trigger {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.data.read(buf)?;
        self.left = self.left.saturating_sub(n);
        if self.left == 0 {
            self.cancel.cancel();
        }
        Ok(n)
    }
}

impl UpdateSource for CancelMidway {
    fn latest(&mut self, _query: &Query<'_>) -> Result<Option<Remote>, UpdateError> {
        Ok(Some(Remote {
            locator: "x".into(),
            size_hint: 0,
        }))
    }

    fn open(
        &mut self,
        _r: &Remote,
        offset: u64,
        limit: Option<u64>,
    ) -> Result<(u64, Box<dyn Read + Send>), UpdateError> {
        let data = std::io::Cursor::new(self.bundle[offset as usize..].to_vec());
        let left = if limit.is_some() { usize::MAX } else { 100_000 };
        Ok((
            offset,
            Box::new(Trigger {
                data,
                left,
                cancel: self.cancel.clone(),
            }),
        ))
    }

    fn is_network(&self) -> bool {
        true
    }
}

struct CancelFactory(Vec<u8>, Mutex<Option<Arc<CancelFlag>>>);

impl SourceFactory for CancelFactory {
    fn open(&self) -> Result<Box<dyn UpdateSource + Send>, UpdateError> {
        let cancel = self.1.lock().unwrap().clone().unwrap();
        Ok(Box::new(CancelMidway {
            bundle: self.0.clone(),
            cancel,
        }))
    }
}

#[test]
fn cancel_stops_a_running_download_and_drops_what_was_fetched() {
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    let factory = Arc::new(CancelFactory(new.bundle.clone(), Mutex::new(None)));
    let events = Arc::new(Collect::default());
    let (core, cancel) = core_on(&device, factory.clone(), events);
    *factory.1.lock().unwrap() = Some(cancel.clone());
    core.accept(
        ADMIN,
        &Job::Download {
            allow_metered: true,
        },
    )
    .unwrap();
    assert!(matches!(
        core.run(Job::Download {
            allow_metered: true
        }),
        Err(UpdateError::Cancelled)
    ));
    let status = core.status().unwrap();
    assert_eq!(status.progress.phase, Phase::Idle);
    assert_eq!(status.progress.error_key, None, "a cancel is not a failure");
    assert!(!device
        .config
        .state_dir
        .join("staging/bundle.part.000")
        .exists());
    assert!(!cancel.is_cancelled(), "the request is consumed");
    assert_eq!(core.history(1)[0].kind_key, "update.history.cancelled");
}

#[test]
fn cancel_while_idle_drops_the_staged_bundle_and_a_pending_restart() {
    let rig = rig();
    let new = release((0, 7, 1), 2);
    rig.publish(&new);
    rig.job(
        ADMIN,
        Job::Download {
            allow_metered: true,
        },
    )
    .unwrap();
    rig.core.cancel(ADMIN).unwrap();
    assert_eq!(rig.core.status().unwrap().progress.phase, Phase::Idle);
    rig.job(
        ADMIN,
        Job::Download {
            allow_metered: true,
        },
    )
    .unwrap();
    rig.job(ADMIN, Job::Apply).unwrap();
    rig.core.cancel(ADMIN).unwrap();
    assert_eq!(rig.core.status().unwrap().progress.phase, Phase::Idle);
    assert_eq!(rig.device.reboot(&[&new]), (Slot::A, 0));
    assert!(!rig.cancel.is_cancelled());
}

#[test]
fn offline_bundles_only_from_configured_folders() {
    let rig = rig();
    let new = release((0, 7, 1), 2);
    let inside = rig.device.write_bundle("new.uup", &new);
    let outside = rig.device.dir.path().join("elsewhere.uup");
    std::fs::write(&outside, &new.bundle).unwrap();
    let job = |path: &Path| Job::Offline {
        path: path.to_path_buf(),
        allow_downgrade: false,
    };
    assert!(matches!(
        rig.core.accept(ADMIN, &job(&outside)),
        Err(UpdateError::PermissionDenied)
    ));
    assert!(matches!(
        rig.core
            .accept(ADMIN, &job(&rig.device.dir.path().join("usb/missing.uup"))),
        Err(UpdateError::InvalidArgument)
    ));
    #[cfg(unix)]
    {
        // A link inside the stick that points outside does not widen access.
        let link = rig.device.dir.path().join("usb/link.uup");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(matches!(
            rig.core.accept(ADMIN, &job(&link)),
            Err(UpdateError::PermissionDenied)
        ));
        let dotdot = rig.device.dir.path().join("usb/../elsewhere.uup");
        assert!(matches!(
            rig.core.accept(ADMIN, &job(&dotdot)),
            Err(UpdateError::PermissionDenied)
        ));
    }
    rig.job(ADMIN, job(&inside)).unwrap();
    let status = rig.core.status().unwrap();
    assert_eq!(status.progress.phase, Phase::Staged);
    assert_eq!(status.progress.version.as_deref(), Some("0.7.1"));
}

#[test]
fn start_reports_fallback_and_previous_version_can_be_chosen() {
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
    let status = rig.core.status().unwrap();
    assert_eq!(
        status.progress.error_key.as_deref(),
        Some("update.error.boot_failed")
    );
    assert_eq!(
        status.progress.phase,
        Phase::Available,
        "nothing staged any more; the release is still on offer"
    );
    // Second try, by hand, works; afterwards the administrator can choose the old version.
    rig.job(ADMIN, DOWNLOAD).unwrap();
    rig.job(ADMIN, Job::Apply).unwrap();
    rig.device.reboot(&[&new]);
    rig.core.resume_after_boot(&Health(true)).unwrap();
    let status = rig.core.status().unwrap();
    assert_eq!(status.current_version, "0.7.1");
    assert!(status.previous_startable);
    assert_eq!(status.previous_version.as_deref(), Some("0.7.0"));
    rig.core.boot_previous(ADMIN).unwrap();
    assert_eq!(rig.device.reboot(&[&new]), (Slot::A, 0));
}
