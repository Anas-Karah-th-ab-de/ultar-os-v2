//! `ab-agent` – drives the real updater on the Ultar kernel for the A/B gate.
//!
//! ```text
//! ab-agent - <daemon> <cli> <config> [uupdate command words ...] [-- more commands ...]
//! ```
//! Starts a bus broker in this process with the sockets `system.updated` and
//! `system.settings` under `/tmp/ab-bus`, starts `<daemon> --config <config>` on the first one,
//! waits until the service answers, then runs `<cli> <command> --socket …/system.settings --lang en` for every command
//! group (groups separated by `--`). Every output line is printed with a prefix the test script
//! greps (`AGENT …`). The daemon is stopped at the end.
//! `ab-agent rawcheck <running-slot-node> <other-slot-node> <table-node>` checks the raw
//! partition rules. Test tooling only.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::{Broker, FixedIdentity, Identity, Policy};

fn broker(dir: &Path) -> Option<Broker> {
    std::fs::create_dir_all(dir).ok()?;
    let mut builder = Broker::builder(Policy::allow_all());
    for name in ["system.updated", "system.settings"] {
        let identity = Identity::new(name).ok()?;
        builder = builder.listen(&dir.join(name), Arc::new(FixedIdentity::new(identity)));
    }
    builder.start().ok()
}

fn run_cli(cli: &str, words: &[String], socket: &Path) -> i32 {
    println!("AGENT RUN {}", words.join(" "));
    let status = Command::new(cli)
        .args(words)
        .arg("--socket")
        .arg(socket)
        .args(["--lang", "en"])
        .status();
    let code = status.map_or(-1, |s| s.code().unwrap_or(-1));
    println!("AGENT EXIT {} {code}", words.join(" "));
    code
}

/// Raw partition rules as the kernel enforces them: the running slot's partition cannot be
/// opened for writing, the other slot's can, the slot table reads back with its magic.
fn rawcheck(running: &Path, other: &Path, table: &Path) {
    let open_rw = |p: &Path| std::fs::OpenOptions::new().read(true).write(true).open(p);
    match open_rw(running) {
        Ok(_) => println!("AGENT RAW running-write allowed"),
        Err(e) => println!(
            "AGENT RAW running-write refused {}",
            e.raw_os_error().unwrap_or(0)
        ),
    }
    match open_rw(other) {
        Ok(f) => {
            use std::io::{Seek, SeekFrom};
            let mut f = f;
            let size = f.seek(SeekFrom::End(0)).unwrap_or(0);
            let synced = f.sync_all().is_ok();
            println!("AGENT RAW other-write ok size {size} fsync {synced}");
        }
        Err(e) => println!(
            "AGENT RAW other-write refused {}",
            e.raw_os_error().unwrap_or(0)
        ),
    }
    let mut magic = [0u8; 4];
    let read = std::fs::File::open(table).and_then(|f| {
        use std::os::unix::fs::FileExt;
        f.read_exact_at(&mut magic, 0)
    });
    println!(
        "AGENT RAW table {}",
        if read.is_ok() && &magic == b"USLT" {
            "USLT"
        } else {
            "unreadable"
        }
    );
}

/// Starts the daemon on the `system.updated` socket and waits until the service answers
/// `status` (at most 60 s); the daemon prints its own state on the console.
fn start_daemon(daemon: &str, cli: &str, config: &str, dir: &Path) -> Option<Child> {
    let mut child = Command::new(daemon)
        .args(["--config", config, "--lang", "en", "--socket"])
        .arg(dir.join("system.updated"))
        .spawn()
        .ok()?;
    let mut ready = false;
    for _ in 0..120 {
        if let Ok(Some(code)) = child.try_wait() {
            println!("AGENT DAEMON exited {code}");
            break;
        }
        let answered = Command::new(cli)
            .arg("status")
            .arg("--socket")
            .arg(dir.join("system.settings"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if answered.is_ok_and(|s| s.success()) {
            ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    println!("AGENT DAEMON {}", if ready { "ready" } else { "not-ready" });
    Some(child)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [cmd, running, other, table] = &args[..] {
        if cmd == "rawcheck" {
            rawcheck(Path::new(running), Path::new(other), Path::new(table));
            return ExitCode::SUCCESS;
        }
    }
    let [_broker_kind, daemon, cli, config, rest @ ..] = &args[..] else {
        println!("AGENT USAGE ab-agent - <daemon> <cli> <config> [commands]");
        return ExitCode::from(2);
    };
    let dir = PathBuf::from("/tmp/ab-bus");
    let Some(_broker) = broker(&dir) else {
        println!("AGENT BROKER FAILED");
        return ExitCode::from(1);
    };
    let Some(mut child) = start_daemon(daemon, cli, config, &dir) else {
        println!("AGENT DAEMON SPAWN FAILED");
        return ExitCode::from(1);
    };
    let mut failed = false;
    for group in rest.split(|w| w == "--").filter(|g| !g.is_empty()) {
        failed |= run_cli(cli, group, &dir.join("system.settings")) != 0;
    }
    let _ = child.kill();
    let _ = child.wait();
    println!("AGENT DONE {}", if failed { "with-failures" } else { "ok" });
    ExitCode::SUCCESS
}
