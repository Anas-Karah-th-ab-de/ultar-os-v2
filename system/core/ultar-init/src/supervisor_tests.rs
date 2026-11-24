//! The supervisor with real processes, started by a stand-in launcher (a
//! shell script that skips the launcher options): order, readiness,
//! restarts, requests, shutdown. Confinement itself is tested with the
//! real launcher in `tests/`.

use super::*;
use crate::groups::GroupTree;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use ultar_bus::messages::init::{Action, Exit};

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ultar-init-sup-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A launcher that only executes the program after `--`.
fn fake_launcher(dir: &std::path::Path) -> PathBuf {
    let path = dir.join("launcher");
    std::fs::write(
        &path,
        "#!/bin/sh\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn spec(name: &str, body: &str) -> ServiceSpec {
    ServiceSpec::parse(body, name).unwrap()
}

fn supervisor(dir: &std::path::Path, specs: Vec<ServiceSpec>) -> Supervisor {
    let env = Environment {
        launcher: fake_launcher(dir),
        groups: GroupTree::none(),
        kernel_rights: false,
        root: false,
        stop_timeout: Duration::from_millis(500),
        edition: "server".into(),
        profile: "system".into(),
        net_allowed: None,
        all_rights_allowed: Vec::new(),
    };
    Supervisor::new(specs, env, Forwarder::new(None), Box::new(|_| {}))
}

/// Reaps only this supervisor's children (tests run in parallel).
fn reap_own(sup: &mut Supervisor) {
    for status in sup.status() {
        if status.pid == 0 {
            continue;
        }
        let pid = nix::unistd::Pid::from_raw(status.pid as i32);
        let exit = match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(_, code)) => Exit::Code(code),
            Ok(WaitStatus::Signaled(_, signal, _)) => Exit::Signal(signal as i32),
            _ => continue,
        };
        sup.on_exit(status.pid, exit, Instant::now());
    }
}

/// Runs the loop until `done` holds (or 10 s passed).
fn drive(sup: &mut Supervisor, mut done: impl FnMut(&Supervisor) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        sup.tick(Instant::now());
        if done(sup) {
            return true;
        }
        let watches: Vec<Watch> = sup.watched().into_iter().map(|(watch, _)| watch).collect();
        {
            let fds: Vec<_> = sup.watched().into_iter().map(|(_, fd)| fd).collect();
            let ready =
                ultar_sys::poll_readable(&fds, Some(Duration::from_millis(20))).unwrap_or_default();
            drop(fds);
            for (watch, readable) in watches.iter().zip(ready) {
                if readable {
                    sup.on_readable(watch, Instant::now());
                }
            }
        }
        reap_own(sup);
    }
    false
}

const READY: &str = "exec = \"/bin/sh\"\nnotify = true\n\
args = [\"-c\", \"sleep 0.2; echo READY $$ > /proc/self/fd/$ULTAR_READY_FD; exec sleep 30\"]\n";

#[test]
fn a_dependant_starts_only_after_its_dependency_reported_ready() {
    let dir = temp("order");
    let late = "exec = \"/bin/sleep\"\nargs = [\"30\"]\nafter = [\"first\"]";
    let mut sup = supervisor(&dir, vec![spec("first", READY), spec("second", late)]);
    sup.tick(Instant::now());
    assert_eq!(sup.state_of("first"), Some(ServiceState::Starting));
    assert_eq!(sup.state_of("second"), Some(ServiceState::Waiting));
    assert!(drive(&mut sup, |s| s.state_of("second")
        == Some(ServiceState::Running)));
    assert_eq!(sup.state_of("first"), Some(ServiceState::Running));
    sup.begin_shutdown(Instant::now());
    assert!(drive(&mut sup, Supervisor::shutdown_done));
    assert!(sup
        .status()
        .iter()
        .all(|s| s.state == ServiceState::Stopped));
}

#[test]
fn a_service_that_never_reports_ready_is_ended_and_retried() {
    let dir = temp("timeout");
    let silent = "exec = \"/bin/sleep\"\nargs = [\"30\"]\nnotify = true\nstart_timeout_ms = 200";
    let mut sup = supervisor(&dir, vec![spec("silent", silent)]);
    assert!(drive(&mut sup, |s| s.status()[0].restarts >= 1));
    assert_eq!(sup.status()[0].last_exit, Some(Exit::Signal(9)));
    sup.begin_shutdown(Instant::now());
    assert!(drive(&mut sup, Supervisor::shutdown_done));
}

#[test]
fn a_crashing_service_backs_off_and_is_given_up_after_the_limit() {
    let dir = temp("crash");
    let crash = "exec = \"/bin/sh\"\nargs = [\"-c\", \"exit 3\"]";
    let mut sup = supervisor(&dir, vec![spec("crash", crash)]);
    // The back-off grows to seconds; the limit is reached after a while.
    let mut seen_backoff = false;
    let failed = drive(&mut sup, |s| {
        seen_backoff |= s.state_of("crash") == Some(ServiceState::Backoff);
        s.state_of("crash") == Some(ServiceState::Failed)
    });
    if !failed {
        // Under load the 10 s may not suffice for all back-offs; the
        // growth itself is tested in `unit.rs`.
        assert!(sup.status()[0].restarts >= 3);
    }
    assert!(seen_backoff);
    assert_eq!(sup.status()[0].last_exit, Some(Exit::Code(3)));
}

#[test]
fn requests_stop_start_and_restart_and_refuse_what_makes_no_sense() {
    let dir = temp("control");
    let body = "exec = \"/bin/sleep\"\nargs = [\"30\"]";
    let mut sup = supervisor(&dir, vec![spec("svc", body)]);
    assert!(drive(&mut sup, |s| s.state_of("svc")
        == Some(ServiceState::Running)));
    let now = Instant::now();
    assert_eq!(
        sup.control(Action::Start, "svc", now),
        Err(ControlError::Already)
    );
    assert_eq!(
        sup.control(Action::Stop, "nope", now),
        Err(ControlError::Unknown)
    );
    assert_eq!(
        sup.control(Action::Stop, "svc", now),
        Ok(ServiceState::Stopping)
    );
    assert!(drive(&mut sup, |s| s.state_of("svc")
        == Some(ServiceState::Stopped)));
    assert_eq!(
        sup.control(Action::Stop, "svc", Instant::now()),
        Err(ControlError::Already)
    );
    sup.control(Action::Start, "svc", Instant::now()).unwrap();
    assert!(drive(&mut sup, |s| s.state_of("svc")
        == Some(ServiceState::Running)));
    let first = sup.status()[0].pid;
    sup.control(Action::Restart, "svc", Instant::now()).unwrap();
    assert!(drive(&mut sup, |s| {
        s.state_of("svc") == Some(ServiceState::Running) && s.status()[0].pid != first
    }));
    assert!(sup.status_text().lines().nth(1).unwrap().starts_with("svc"));
    sup.begin_shutdown(Instant::now());
    assert_eq!(
        sup.control(Action::Start, "svc", Instant::now()),
        Err(ControlError::ShuttingDown)
    );
    assert!(drive(&mut sup, Supervisor::shutdown_done));
}

#[test]
fn a_service_that_ignores_the_stop_signal_gets_the_hard_end_after_its_deadline() {
    let dir = temp("stubborn");
    let body = "exec = \"/bin/sh\"\nstop_timeout_ms = 300\n\
                args = [\"-c\", \"trap '' TERM; while :; do sleep 1; done\"]";
    let mut sup = supervisor(&dir, vec![spec("stubborn", body)]);
    assert!(drive(&mut sup, |s| s.state_of("stubborn")
        == Some(ServiceState::Running)));
    // Time for the shell to set its trap.
    std::thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    sup.begin_shutdown(started);
    assert!(drive(&mut sup, Supervisor::shutdown_done));
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert_eq!(sup.status()[0].last_exit, Some(Exit::Signal(9)));
}

#[test]
fn output_lines_are_collected_under_the_service_name() {
    let dir = temp("output");
    let body =
        "exec = \"/bin/sh\"\nrestart = \"never\"\nargs = [\"-c\", \"echo out; echo err >&2\"]";
    let mut sup = supervisor(&dir, vec![spec("talk", body)]);
    assert!(drive(&mut sup, |s| s.state_of("talk") == Some(ServiceState::Done)));
    // init's own lines and the two of the service wait for the log service.
    assert!(sup.forwarder.waiting() >= 2);
}

#[test]
fn a_dependency_cycle_is_refused_and_the_rest_runs() {
    let dir = temp("cycle");
    let mut sup = supervisor(
        &dir,
        vec![
            spec(
                "a",
                "exec = \"/bin/sleep\"\nargs = [\"30\"]\nafter = [\"b\"]",
            ),
            spec(
                "b",
                "exec = \"/bin/sleep\"\nargs = [\"30\"]\nafter = [\"a\"]",
            ),
            spec("c", "exec = \"/bin/sleep\"\nargs = [\"30\"]"),
        ],
    );
    assert!(drive(&mut sup, |s| s.state_of("c") == Some(ServiceState::Running)));
    assert_eq!(sup.state_of("a"), Some(ServiceState::Failed));
    sup.begin_shutdown(Instant::now());
    assert!(drive(&mut sup, Supervisor::shutdown_done));
}
