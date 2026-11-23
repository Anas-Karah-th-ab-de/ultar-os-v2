//! Review round 3 for the device daemon (same runtime and start sequence as
//! the settings service): a stop while the broker is awaited ends the
//! daemon at once with exit 0 (R3-S1), and a token descriptor that was not
//! passed stops the start instead of reading the runtime's own descriptor
//! (R3-S2).

mod rig;

use rig::Rig;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const DAEMON: &str = env!("CARGO_BIN_EXE_ultar-devd");

fn daemon(rig: &Rig, extra: &[&str]) -> Child {
    let text = |path: &std::path::Path| path.to_string_lossy().into_owned();
    let bus = rig.root.parent().unwrap().join("bus.sock");
    let mut command = Command::new(DAEMON);
    command
        .args(["--depot", &text(&rig.depot), "--keys", &text(&rig.keys)])
        .args(["--state", &text(&rig.state), "--root", &text(&rig.root)])
        .args(["--bus", &text(&bus), "--lang", "en", "--allow-all"])
        .args(extra)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if ultar_service::effective_uid() == 0 {
        command.arg("--allow-root");
    }
    command.spawn().unwrap()
}

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

fn kill(pid: u32, signal: &str) {
    let status = Command::new("kill")
        .args([&format!("-{signal}"), &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn sigterm_while_waiting_for_the_broker_stops_at_once_with_zero() {
    let rig = Rig::new("stop-early");
    let child = daemon(&rig, &[]);
    std::thread::sleep(Duration::from_millis(400));
    kill(child.id(), "TERM");
    let (code, took) = wait_exit(child, Duration::from_secs(20));
    assert_eq!(code, Some(0), "72 = stop deadline");
    assert!(took < Duration::from_secs(1), "stop took {took:?}");
}

#[test]
fn a_token_descriptor_that_was_not_passed_stops_the_start() {
    let rig = Rig::new("token-fd");
    let child = daemon(&rig, &["--token-fd", "3"]);
    let (code, _) = wait_exit(child, Duration::from_secs(20));
    assert!(matches!(code, Some(code) if code != 0), "{code:?}");
}
