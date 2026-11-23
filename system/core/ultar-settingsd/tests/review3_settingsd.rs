//! Review round 3 (service runtime): regression tests for confirmed defects
//! seen through `ultar-settingsd`. Target path:
//! `system/core/ultar-settingsd/tests/review3_settingsd.rs`.
//!
//! Every test here FAILS on commit 157060b and must pass once the defect
//! named in its comment is fixed. (`ultar-devd` shares both defects: same
//! runtime, same start sequence.)

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_bus::{Broker, FixedIdentity, Identity, Policy};
use ultar_settingsd::testdir::TestDir;

const DAEMON: &str = env!("CARGO_BIN_EXE_ultar-settingsd");

fn send_signal(pid: u32, signal: &str) {
    let status = Command::new("kill")
        .args([&format!("-{signal}"), &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success());
}

fn daemon(dir: &Path, extra: &[&str]) -> Child {
    let mut command = Command::new(DAEMON);
    command
        .arg("--store")
        .arg(dir.join("store"))
        .arg("--socket")
        .arg(dir.join("bus.sock"))
        .args(["--lang", "en", "--allow-all"])
        .args(extra)
        .env_remove("ULTAR_BUS_SOCKET")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if ultar_service::effective_uid() == 0 {
        command.arg("--allow-root");
    }
    command.spawn().unwrap()
}

/// Exit code and run time; the child is killed after `limit` (code `None`).
fn wait_exit(mut child: Child, limit: Duration) -> (Option<i32>, Duration) {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return (status.code(), start.elapsed());
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return (None, start.elapsed());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// R3-S1: while the daemon waits for the broker at start (up to 10 s) a
/// stop signal is not acted on. Nothing reacts until the stop deadline ends
/// the process with code 72 after 2.5 s – a failed stop for the supervisor
/// and 2.5 s lost for every such service at shutdown.
#[test]
fn sigterm_while_waiting_for_the_broker_stops_at_once_and_cleanly() {
    let dir = TestDir::new("r3-term-early");
    let child = daemon(dir.path(), &[]);
    std::thread::sleep(Duration::from_millis(400));
    send_signal(child.id(), "TERM");
    let (code, took) = wait_exit(child, Duration::from_secs(20));
    assert_eq!(code, Some(0), "exit code (72 = stop deadline)");
    assert!(took < Duration::from_secs(1), "stop took {took:?}");
}

/// R3-S2: `--token-fd N` with a number that was not inherited adopts
/// whatever the runtime itself opened under that number – with 3 its own
/// signal descriptor. The daemon then reads its "token" from the signal
/// descriptor: it never becomes ready and swallows SIGTERM. `adopt_fd` is a
/// safe function that hands out a second owner of a live descriptor.
#[test]
fn a_token_descriptor_that_was_not_inherited_is_refused_at_start() {
    let dir = TestDir::new("r3-token-fd");
    let identity = Identity::new("test.peer").unwrap();
    let _broker = Broker::builder(Policy::allow_all())
        .listen(
            &dir.path().join("bus.sock"),
            Arc::new(FixedIdentity::new(identity)),
        )
        .start()
        .unwrap();
    let child = daemon(dir.path(), &["--token-fd", "3"]);
    std::thread::sleep(Duration::from_millis(1500));
    send_signal(child.id(), "TERM");
    let (code, took) = wait_exit(child, Duration::from_secs(10));
    assert!(
        matches!(code, Some(code) if code != 0),
        "expected a start error, got {code:?} after {took:?} (None = had to be killed)"
    );
}
