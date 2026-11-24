//! The checks of `launchtest` (see there).

use super::{apps, install, output, start, wait_app, Packages, Run, CALL};
use std::os::unix::fs::MetadataExt;
use ultar_bus::messages::launcher::{Exit, InstanceState as S, Role, Stop, Uninstall};
use ultar_bus::Client;

/// Waits until the launcher answers on the bus.
fn wait_ready(admin: &Client) -> Result<(), String> {
    let deadline = std::time::Instant::now() + super::WAIT;
    while apps(admin).is_err() {
        if std::time::Instant::now() > deadline {
            return Err("launcher not on the bus".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    Ok(())
}

/// A well-behaved app: its own user, its view, its output in the log.
fn good(run: &Run, packages: &Packages, admin: &Client) -> Result<(), String> {
    let uid = install(admin, packages.make(&run.bin, "good", &["info"], "", true)?)?;
    if uid != 20000 {
        return Err(format!("good got uid {uid}"));
    }
    start(admin, "good")?;
    let exit = wait_app(admin, "good", &[S::Stopped, S::Crashed])?;
    let line = output(admin, "good", "TESTAPP uid=")?;
    println!("LAUNCHER: good: {line}");
    let rights = if ultar_sys::ultar_rights().is_ok() {
        "rights=0 "
    } else {
        "rights=none "
    };
    for want in [
        "uid=20000 gid=20000",
        rights,
        "cwd=/data",
        "data_writable=true",
        "tmp_writable=true",
        "root_readonly=true",
        "token=5",
        "app=good",
    ] {
        if !line.contains(want) {
            return Err(format!("good: {want:?} missing in {line:?}"));
        }
    }
    output(admin, "good", "TESTAPP stderr line")?;
    let meta = std::fs::metadata(run.at("data/appdata/good")).map_err(|e| e.to_string())?;
    if (meta.uid(), meta.mode() & 0o7777) != (20000, 0o700) || exit != Some(Exit::Code(0)) {
        return Err(format!(
            "good: data owner {} mode {:o}, end {exit:?}",
            meta.uid(),
            meta.mode()
        ));
    }
    Ok(())
}

/// Every attack the malicious app reported (shown in the run's log).
fn report_attacks(admin: &Client, groups: bool) -> Result<(), String> {
    let mut names = vec![
        "ATTACK read_other_data ",
        "ATTACK kill_other_app ",
        "ATTACK become_root ",
        "ATTACK kernel_rights ",
        "ATTACK new_namespace ",
        "ATTACK mount ",
        "ATTACK write_system ",
        "ATTACK write_root ",
        "ATTACK privileged_socket ",
        "ATTACK fork_bomb ",
        "FORKS ",
    ];
    if groups {
        names.extend(["ATTACK fill_memory ", "MEMORY "]);
    }
    for name in names {
        println!("LAUNCHER: evil: {}", output(admin, "store.evil", name)?);
    }
    Ok(())
}

/// A malicious app against its cell and another app.
fn evil(run: &Run, packages: &Packages, admin: &Client) -> Result<(), String> {
    install(admin, packages.make(&run.bin, "other", &["run"], "", true)?)?;
    let victim = start(admin, "other")?;
    std::fs::write(run.at("data/appdata/other/secret"), "victim").map_err(|e| e.to_string())?;
    let data = run.at("data/appdata/other").display().to_string();
    let pid = victim.to_string();
    let admin_socket = run.at("run/bus/system.sock").display().to_string();
    let log_socket = run.at("run/log/init.sock").display().to_string();
    let mut args = vec!["attack", "--victim-data", &data, "--victim-pid", &pid];
    args.extend([
        "--socket",
        &admin_socket,
        "--socket",
        &log_socket,
        "--forks",
        "64",
    ]);
    if run.groups {
        args.push("--memory");
    }
    let limits = "[limits]\npids = 16\nmemory_mb = 64\n";
    let uid = install(
        admin,
        packages.make(&run.bin, "store.evil", &args, limits, false)?,
    )?;
    start(admin, "store.evil")?;
    let exit = wait_app(admin, "store.evil", &[S::Stopped, S::Crashed])?;
    let verdict = output(admin, "store.evil", "ATTACKS:")?;
    report_attacks(admin, run.groups)?;
    if verdict != "ATTACKS: ALL BLOCKED" || exit != Some(Exit::Code(0)) {
        return Err(format!("evil (uid {uid}): {verdict} end {exit:?}"));
    }
    let other = apps(admin)?
        .into_iter()
        .find(|a| a.app == "other")
        .and_then(|a| a.instances.first().map(|i| i.state));
    let secret = std::fs::read_to_string(run.at("data/appdata/other/secret")).unwrap_or_default();
    if other != Some(S::Running) || secret != "victim" {
        return Err(format!("other after the attack: {other:?} {secret:?}"));
    }
    Ok(())
}

/// The limits of the app's group (Ultar kernel group tree).
fn groups(run: &Run) -> Result<(), String> {
    if !run.groups {
        return Ok(());
    }
    let dir = std::path::Path::new(ultar_sys::machine::GROUP_ROOT).join("apps/store.evil");
    let read = |f: &str| {
        std::fs::read_to_string(dir.join(f))
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    let (memory, pids) = (read("memory.max"), read("pids.max"));
    println!("LAUNCHER: group apps/evil memory.max={memory} pids.max={pids}");
    if memory != (64u64 << 20).to_string() || pids != "16" {
        return Err(format!(
            "group apps/evil: memory.max {memory} pids.max {pids}"
        ));
    }
    Ok(())
}

/// Stop, uninstall, and a reinstall gets its id back.
fn lifecycle(run: &Run, packages: &Packages, admin: &Client) -> Result<(), String> {
    admin
        .call(
            &Stop {
                app: "other".into(),
                role: Role::Ui,
            },
            CALL,
        )
        .map_err(|e| format!("stop {e}"))?;
    let exit = wait_app(admin, "other", &[S::Stopped])?;
    admin
        .call(
            &Uninstall {
                app: "good".into(),
                keep_data: false,
            },
            CALL,
        )
        .map_err(|e| format!("uninstall {e}"))?;
    let third = install(
        admin,
        packages.make(&run.bin, "store.third", &["info"], "", false)?,
    )?;
    let again = install(admin, packages.make(&run.bin, "good", &["info"], "", true)?)?;
    println!("LAUNCHER: other stopped ({exit:?}); third uid {third}, good again uid {again}");
    if third != 20003 || again != 20000 {
        return Err(format!("ids: third {third}, good again {again}"));
    }
    Ok(())
}

/// Every check, in order.
pub fn all(run: &Run, packages: &Packages, admin: &Client) -> Result<(), String> {
    wait_ready(admin)?;
    good(run, packages, admin)?;
    evil(run, packages, admin)?;
    groups(run)?;
    lifecycle(run, packages, admin)
}
