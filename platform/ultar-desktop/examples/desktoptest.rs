//! End-to-end run of the desktop session with the shipped definitions: `ultar-init` (not
//! PID 1) starts the log service, the bus, the seat helper, the authentication service, the
//! compositor, the lock screen, the system dialogs and the desktop shell from the **shipped
//! service files** (paths moved below a work directory; `manual` and `editions` dropped so that
//! they start at once). Before that a test account gets its password through
//! `ultar-authd passwd` on standard input. The run checks that every service runs as its own
//! user, then prints `DESKTOP-TREIBER: BEREIT` and keeps the session up for `--hold` seconds:
//! the host (`tools/kern-test/desktop-qemu.py`) locks with Super+L, types the password through
//! the virtual keyboard and judges screenshots. Last line `DESKTOP-TREIBER: ENDE` or
//! `DESKTOP-TREIBER: FEHLER <what>`.
//!
//! ```text
//! desktoptest --bin DIR --work DIR [--group-root DIR] [--hold SECONDS] [--password-file FILE]
//! ```
//!
//! Runs as root on the Ultar kernel (`tools/kern-test/desktop-e2e.sh`) and on Linux.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use ultar_bus::messages::init::{ServiceState, Status};
use ultar_bus::{Client, ClientOptions};

/// The services of the session and the user each must run as (rootfs/etc/ultar/ids.toml).
const SERVICES: [(&str, &str, u32); 8] = [
    (
        "logd",
        include_str!("../../../rootfs/etc/ultar/services/logd.toml"),
        802,
    ),
    (
        "bus",
        include_str!("../../../rootfs/etc/ultar/services/bus.toml"),
        801,
    ),
    (
        "seat",
        include_str!("../../../rootfs/etc/ultar/services/seat.toml"),
        0,
    ),
    (
        "authd",
        include_str!("../../../rootfs/etc/ultar/services/authd.toml"),
        813,
    ),
    (
        "compositor",
        include_str!("../../../rootfs/etc/ultar/services/compositor.toml"),
        811,
    ),
    (
        "lock",
        include_str!("../../../rootfs/etc/ultar/services/lock.toml"),
        814,
    ),
    (
        "sysdialog",
        include_str!("../../../rootfs/etc/ultar/services/sysdialog.toml"),
        815,
    ),
    (
        "desktop",
        include_str!("../../../rootfs/etc/ultar/services/desktop.toml"),
        816,
    ),
];
/// Configuration files the services read, by their name below `/etc/ultar/`.
const ETC: [(&str, &str); 3] = [
    (
        "bus-policy.toml",
        include_str!("../../../rootfs/etc/ultar/bus-policy.toml"),
    ),
    (
        "compositor-rights.toml",
        include_str!("../../../rootfs/etc/ultar/compositor-rights.toml"),
    ),
    (
        "devd-seat.toml",
        include_str!("../../../rootfs/etc/ultar/devd-seat.toml"),
    ),
];
/// The test account.
const USER: &str = "tester";
/// Its password unless `--password-file` names another (digits: the same keys in every layout).
const PASSWORD: &str = "24680135792";
const WAIT: Duration = Duration::from_secs(240);
const CALL: Duration = Duration::from_secs(30);

struct Run {
    bin: PathBuf,
    work: PathBuf,
    groups: Option<PathBuf>,
}

impl Run {
    fn at(&self, path: &str) -> PathBuf {
        self.work.join(path)
    }

    /// A shipped file with its paths moved below the work directory.
    fn moved(&self, text: &str) -> String {
        let work = self.work.display().to_string();
        text.replace("/run/ultar/", &format!("{work}/run/"))
            .replace("/data/ultar/", &format!("{work}/data/"))
            .replace("/etc/ultar/", &format!("{work}/etc/"))
            .replace("/usr/bin/", &format!("{}/", self.bin.display()))
    }

    /// The shipped definition, moved, started at once on any edition.
    fn service(&self, text: &str) -> String {
        self.moved(text)
            .lines()
            .filter(|l| !l.starts_with("manual") && !l.starts_with("editions"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Where the compositor writes how many key events it took.
    fn ack(&self) -> PathBuf {
        self.at("run/ui/input-ack")
    }

    fn layout(&self) -> std::io::Result<PathBuf> {
        let services = self.at("etc/services");
        std::fs::create_dir_all(&services)?;
        for (name, text, _) in SERVICES {
            let mut service = self.service(text);
            if name == "compositor" {
                // The test's input acknowledgement: the compositor counts the key events it
                // took (tools/kern-test/desktop-qemu.py waits for each key instead of guessing).
                let ack = format!("\"--input-ack\", \"{}\",\n]", self.ack().display());
                service = service.replacen("\n]", &format!("\n{ack}"), 1);
            }
            std::fs::write(services.join(format!("{name}.toml")), service)?;
        }
        for (name, text) in ETC {
            std::fs::write(self.at(&format!("etc/{name}")), self.moved(text))?;
        }
        // The group tree of the machine (the kernel's or Linux's cgroup root), given by the host.
        let group_root = match &self.groups {
            Some(root) => format!("group_root = \"{}\"\n", root.display()),
            None => String::new(),
        };
        let config = self.at("etc/init.toml");
        std::fs::write(
            &config,
            format!(
                "shutdown_grace_ms = 5000\nservice_dirs = [\"{}\"]\noutput_socket = \"{}\"\nbus_socket = \"{}\"\n\
                 bus_policy = \"{}\"\ncontrol_socket = \"{}\"\n{group_root}",
                services.display(),
                self.at("run/log/init.sock").display(),
                self.at("run/bus/init.sock").display(),
                self.at("etc/bus-policy.toml").display(),
                self.at("ctl.sock").display(),
            ),
        )?;
        Ok(config)
    }

    /// The test account, its password typed twice on standard input (never an argument).
    fn account(&self, password: &str) -> Result<(), String> {
        let dir = self.at("data/auth");
        std::fs::create_dir_all(&dir).map_err(|e| format!("auth dir {e}"))?;
        let mut child = Command::new(self.bin.join("ultar-authd"))
            .args(["--dir"])
            .arg(&dir)
            .args(["passwd", USER, "--display", "Tester", "--lang", "en"])
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("authd passwd {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            writeln!(stdin, "{password}\n{password}").map_err(|e| format!("stdin {e}"))?;
        }
        let status = child.wait().map_err(|e| format!("passwd {e}"))?;
        if !status.success() {
            return Err(format!("passwd ended {status}"));
        }
        // The service owns its directory (it runs as 813).
        own_tree(&dir, 813)
    }
}

fn own_tree(path: &Path, uid: u32) -> Result<(), String> {
    std::os::unix::fs::chown(path, Some(uid), Some(uid)).map_err(|e| format!("chown {e}"))?;
    if path.is_dir() {
        for entry in std::fs::read_dir(path).map_err(|e| format!("read_dir {e}"))? {
            own_tree(&entry.map_err(|e| e.to_string())?.path(), uid)?;
        }
    }
    Ok(())
}

/// Waits until every service of the session runs, each as its user.
fn wait_running(admin: &Client) -> Result<(), String> {
    let deadline = Instant::now() + WAIT;
    loop {
        // init registers its bus endpoint a moment after the bus is up
        let status = match admin.call(&Status {}, CALL) {
            Ok(status) => status,
            Err(e) if Instant::now() > deadline => return Err(format!("init.status {e}")),
            Err(_) => {
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        let missing: Vec<String> = SERVICES
            .iter()
            .filter(|(name, _, uid)| {
                !status.services.iter().any(|s| {
                    s.name == *name
                        && s.state == ServiceState::Running
                        && (*uid == 0 || s.uid == *uid)
                })
            })
            .map(|(name, _, _)| (*name).to_owned())
            .collect();
        if missing.is_empty() {
            for s in &status.services {
                println!(
                    "desktoptest: {} pid={} uid={} running",
                    s.name, s.pid, s.uid
                );
            }
            return Ok(());
        }
        if Instant::now() > deadline {
            let seen: Vec<String> = status
                .services
                .iter()
                .map(|s| format!("{}={:?}/{}", s.name, s.state, s.uid))
                .collect();
            return Err(format!(
                "not running as their users: {missing:?} ({seen:?})"
            ));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Prints `EINGABE: <n>` each time the compositor's count of key events changes (the host
/// waits for it after every key it sends; a count only – never which key).
fn relay_acks(path: PathBuf) {
    let _ = std::thread::Builder::new()
        .name("acks".into())
        .spawn(move || {
            let mut last = String::new();
            loop {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let now = text.trim().to_owned();
                    if now != last && !now.is_empty() {
                        println!("EINGABE: {now}");
                        last = now;
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
}

/// Ends the session: init and, with it, its services (the host has its verdict by now).
fn stop_init(mut init: Child) -> Result<(), String> {
    init.kill().map_err(|e| e.to_string())?;
    let deadline = Instant::now() + WAIT;
    loop {
        match init.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if Instant::now() > deadline => return Err("init did not stop".into()),
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn test(run: &Run, hold: Duration, password: &str) -> Result<(), String> {
    let config = run.layout().map_err(|e| format!("layout {e}"))?;
    run.account(password)?;
    let init = Command::new(run.bin.join("ultar-init"))
        .arg("--config")
        .arg(config)
        .spawn()
        .map_err(|e| format!("init {e}"))?;
    let options = ClientOptions {
        connect_retry_for: WAIT,
        ..ClientOptions::named("desktoptest")
    };
    let checked = Client::connect(&run.at("run/bus/system.sock"), options)
        .map_err(|e| format!("bus {e}"))
        .and_then(|admin| {
            wait_running(&admin)?;
            println!("DESKTOP-TREIBER: BEREIT");
            relay_acks(run.ack());
            let end = Instant::now() + hold;
            while Instant::now() < end {
                std::thread::sleep(Duration::from_secs(5));
                // Still up: the lock screen and the shell must survive the user's steps.
                let status = admin
                    .call(&Status {}, CALL)
                    .map_err(|e| format!("status {e}"))?;
                let restarted: Vec<&str> = status
                    .services
                    .iter()
                    .filter(|s| s.restarts > 0)
                    .map(|s| s.name.as_str())
                    .collect();
                if !restarted.is_empty() {
                    return Err(format!("restarted: {restarted:?}"));
                }
                println!("desktoptest: alive");
            }
            Ok(())
        });
    let stopped = stop_init(init);
    checked.and(stopped)
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        let index = words.iter().position(|word| word == name)?;
        words.get(index + 1).cloned()
    };
    let (Some(bin), Some(work)) = (value("--bin"), value("--work")) else {
        println!(
            "DESKTOP-TREIBER: FEHLER usage: desktoptest --bin DIR --work DIR [--group-root DIR] [--hold S]"
        );
        return;
    };
    let hold = value("--hold")
        .and_then(|s| s.parse().ok())
        .map_or(Duration::from_secs(600), Duration::from_secs);
    let password = value("--password-file")
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map_or(PASSWORD.to_owned(), |s| s.trim().to_owned());
    let groups = value("--group-root").map(PathBuf::from);
    let run = Run {
        bin: PathBuf::from(bin),
        work: PathBuf::from(work),
        groups,
    };
    match test(&run, hold, &password) {
        Ok(()) => println!("DESKTOP-TREIBER: ENDE"),
        Err(what) => println!("DESKTOP-TREIBER: FEHLER {what}"),
    }
}
