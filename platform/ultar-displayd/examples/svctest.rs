//! End-to-end run of the display and account services under the service
//! manager: `ultar-init` (not PID 1) starts the **shipped** definitions of
//! logd, bus, settingsd, displayd and accountd (paths moved below a work
//! directory) and a headless compositor with the shipped rights file, each
//! as its own user. Stand-ins answer for the authentication service and the
//! system dialog (not part of this tree). Checked: the displays reach the
//! bus with their identity, Super+P waits for "Keep?", killing the display
//! service takes the change back at once in the compositor and – after init
//! restarted it – in the settings; the first administrator gets a home
//! owned by the new id, a second user needs the dialog. Last line:
//! `SVC: ALLES OK` or `SVC: FEHLER <what>`.
//!
//! ```text
//! svctest --bin DIR --work DIR [--group-root DIR] [--no-file-admin]
//! ```
//! `--no-file-admin`: the kernel gives accountd no right to hand a home to
//! another id (Linux without capabilities): the run then expects the first
//! administrator to be refused with `home_failed` and nothing left behind.

#[path = "svctest/peers.rs"]
mod peers;
#[path = "svctest/setup.rs"]
mod setup;

use setup::Run;
use std::os::unix::fs::MetadataExt;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use ultar_bus::messages::account_local::{CreateUser, Secret, UserKind};
use ultar_bus::messages::display_service::{Apply, Change, Outputs, Projection};
use ultar_bus::messages::init::{ServiceState, Status};
use ultar_bus::messages::settings::{Get, SettingValue};
use ultar_bus::Client;
use ultar_surface::proto::Event;

const WAIT: Duration = Duration::from_secs(120);

fn wait<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> Result<T, String> {
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

fn running(admin: &Client) -> Vec<(String, u32, u32)> {
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

/// `display.mode` as the administrator's tool reads it.
fn mode(admin: &Client) -> Option<SettingValue> {
    admin
        .call(
            &Get {
                key: "display.mode".into(),
            },
            Duration::from_secs(5),
        )
        .ok()
        .and_then(|r| r.value)
}

/// Outputs the compositor shows (a plain client sees only enabled ones).
fn shown(run: &Run) -> Option<usize> {
    let mut c =
        ultar_surface::client::Client::connect(&run.at("run/ui/compositor.sock"), "svctest")
            .ok()?;
    let mut n = 0;
    while let Ok(Some(e)) = c.next_event(Some(Duration::from_millis(300))) {
        if matches!(e, Event::Output(_)) {
            n += 1;
        }
    }
    Some(n)
}

fn displays(run: &Run, app: &Client) -> Result<(), String> {
    let outputs = app
        .subscribe::<Outputs>()
        .map_err(|e| format!("subscribe {e}"))?;
    let ids = wait("display.outputs", || {
        let o = outputs.recv_timeout(Duration::from_secs(1)).ok()?;
        (o.outputs.len() == 2).then(|| o.outputs.iter().map(|x| x.id.clone()).collect::<Vec<_>>())
    })?;
    if ids != ["uhl-0001-1", "uhl-0001-2"] {
        return Err(format!("display ids {ids:?}"));
    }
    wait("both shown", || (shown(run)? == 2).then_some(()))?;
    let apply = Apply {
        change: Change::Projection(Projection::SecondOnly),
    };
    app.call(&apply, Duration::from_secs(10))
        .map_err(|e| format!("apply {e}"))?;
    wait("only the second shown", || (shown(run)? == 1).then_some(()))?;
    println!("SVC: Super+P second display only, waiting for Keep?");
    Ok(())
}

fn kill_displayd(admin: &Client, run: &Run) -> Result<(), String> {
    let (_, uid, pid) = running(admin)
        .into_iter()
        .find(|s| s.0 == "displayd")
        .ok_or("displayd not running")?;
    if uid != 817 {
        return Err(format!("displayd runs as {uid}"));
    }
    let pid = rustix::process::Pid::from_raw(pid as i32).ok_or("pid")?;
    rustix::process::kill_process(pid, rustix::process::Signal::KILL).map_err(|e| e.to_string())?;
    wait("compositor went back at once", || {
        (shown(run)? == 2).then_some(())
    })?;
    println!("SVC: display service killed, compositor back to both displays");
    let extend = SettingValue::Text("extend".into());
    wait("settings taken back after the restart", || {
        (mode(admin)? == extend).then_some(())
    })?;
    println!("SVC: restarted display service took the waiting change back");
    Ok(())
}

fn accounts(run: &Run, dialog: &peers::Peers) -> Result<(), String> {
    let installer = run.client("system.installer")?;
    let app = run.client("app.settings")?;
    let user = |name: &str, kind| CreateUser {
        name: name.into(),
        display_name: name.into(),
        kind,
        password: Secret::new("sesame-1".into()),
        confirm: Secret::new("sesame-1".into()),
    };
    let reply = installer
        .call(
            &user("anas", UserKind::Administrator),
            Duration::from_secs(30),
        )
        .map_err(|e| format!("create {e}"))?
        .outcome;
    if !run.file_admin {
        return match reply {
            Err(r)
                if r.reason.key == "accountd.refused.home_failed"
                    && !run.at("home/anas").exists() =>
            {
                println!(
                    "SVC: without file_admin the first user is refused (home_failed), nothing left"
                );
                Ok(())
            }
            other => Err(format!("without file_admin: {other:?}")),
        };
    }
    let first = reply.map_err(|r| format!("create refused {}", r.reason.key))?;
    let meta = std::fs::metadata(run.at("home/anas")).map_err(|e| format!("home {e}"))?;
    if first.users[0].uid != 1000 || meta.uid() != 1000 || meta.mode() & 0o777 != 0o700 {
        return Err(format!("home owner {} mode {:o}", meta.uid(), meta.mode()));
    }
    let refused = app.call(&user("leila", UserKind::Standard), Duration::from_secs(30));
    if !matches!(refused, Ok(ref r) if r.outcome.is_err()) || dialog.asked() != 1 {
        return Err("a second user without the dialog".into());
    }
    dialog.allow_next();
    let made = app.call(&user("leila", UserKind::Standard), Duration::from_secs(30));
    match made.map(|r| r.outcome) {
        Ok(Ok(u)) if u.users.len() == 2 => {}
        other => return Err(format!("second user {other:?}")),
    }
    println!("SVC: first administrator 1000 with its home, second user after the dialog");
    Ok(())
}

fn stop(init: &Child) -> Result<(), String> {
    let pid = rustix::process::Pid::from_raw(init.id() as i32).ok_or("pid")?;
    rustix::process::kill_process(pid, rustix::process::Signal::TERM).map_err(|e| e.to_string())
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
        println!("SVC: running {:?}", running(&admin));
        let peers = peers::Peers::start(run)?;
        let app = run.client("app.settings")?;
        displays(run, &app)?;
        kill_displayd(&admin, run)?;
        accounts(run, &peers)?;
        Ok(())
    })();
    let stopped = stop(&init);
    let ended = wait("init ended", || init.try_wait().ok().flatten());
    checked
        .and(stopped)
        .and(ended.and_then(|s| s.success().then_some(()).ok_or(format!("init ended {s}"))))
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        let index = words.iter().position(|word| word == name)?;
        words.get(index + 1).map(std::path::PathBuf::from)
    };
    let (Some(bin), Some(work)) = (value("--bin"), value("--work")) else {
        println!(
            "SVC: FEHLER usage: svctest --bin DIR --work DIR [--group-root DIR] [--no-file-admin]"
        );
        return;
    };
    let group_root = value("--group-root").map(|p| p.display().to_string());
    let file_admin = !words.iter().any(|word| word == "--no-file-admin");
    let run = Run {
        bin,
        work,
        file_admin,
    };
    match test(&run, group_root.as_deref()) {
        Ok(()) => println!("SVC: ALLES OK"),
        Err(what) => println!("SVC: FEHLER {what}"),
    }
}
