//! End-to-end run of the system services behind the desktop shell under the service
//! manager: `ultar-init` (not PID 1) starts the **shipped** definitions of logd, bus,
//! authd, sessiond, powerd, notifyd, calendard and audiod (paths moved below a work
//! directory), each as its own user. The test plays the shell, the lock screen (it
//! answers `lock.lock`), an app with notifications and the calendar app. Checked: a
//! sign-in through the credential store, notification buttons back to their sender
//! only, calendar events of the person in front, the honest "no device" of the audio
//! service and of sleep where the machine has none, a killed service restarted by init,
//! signing out, and power off through `init.power` (every service ends with code 0).
//! Last line: `DIENSTE: ALLES OK` or `DIENSTE: FEHLER <what>`.
//!
//! ```text
//! dienstetest --bin DIR --work DIR [--group-root DIR] [--fake-root]
//! ```

#[path = "setup.rs"]
mod setup;
#[path = "steps.rs"]
mod steps;

use setup::Run;
use std::process::Command;
use std::time::{Duration, Instant};
use ultar_bus::messages::init::{ServiceState, Status};
use ultar_bus::Client;

pub const WAIT: Duration = Duration::from_secs(120);

pub fn wait<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> Result<T, String> {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(v) = f() {
            return Ok(v);
        }
        if Instant::now() > deadline {
            return Err(what.to_owned());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub fn running(admin: &Client) -> Vec<(String, u32, u32)> {
    admin
        .call(&Status {}, Duration::from_secs(5))
        .map(|r| {
            r.services
                .into_iter()
                .filter(|s| s.state == ServiceState::Running)
                .map(|s| (s.name, s.uid, s.pid))
                .collect()
        })
        .unwrap_or_default()
}

fn test(run: &Run, group_root: Option<&str>) -> Result<(), String> {
    let config = run.layout(group_root).map_err(|e| format!("layout {e}"))?;
    let mut init = Command::new(run.bin.join("ultar-init"))
        .arg("--config")
        .arg(config)
        .spawn()
        .map_err(|e| format!("init {e}"))?;
    let checked = (|| {
        let admin = run.client("system.admin")?;
        wait("services running", || {
            (running(&admin).len() == setup::SERVICES.len()).then_some(())
        })?;
        println!("DIENSTE: running {:?}", running(&admin));
        steps::all(run, &admin)
    })();
    if checked.is_err() {
        let pid = rustix::process::Pid::from_raw(init.id() as i32).ok_or("pid")?;
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
    }
    let ended = wait("init ended", || init.try_wait().ok().flatten());
    checked.and(ended.and_then(|s| s.success().then_some(()).ok_or(format!("init ended {s}"))))
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        let index = words.iter().position(|word| word == name)?;
        words.get(index + 1).map(std::path::PathBuf::from)
    };
    let (Some(bin), Some(work)) = (value("--bin"), value("--work")) else {
        println!("DIENSTE: FEHLER usage: dienstetest --bin DIR --work DIR [--group-root DIR] [--fake-root]");
        return;
    };
    let group_root = value("--group-root").map(|p| p.display().to_string());
    let fake_root = words.iter().any(|w| w == "--fake-root");
    let run = Run {
        bin,
        work,
        fake_root,
    };
    match test(&run, group_root.as_deref()) {
        Ok(()) => println!("DIENSTE: ALLES OK"),
        Err(what) => println!("DIENSTE: FEHLER {what}"),
    }
}
