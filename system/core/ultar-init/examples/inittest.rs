//! End-to-end run of the service manager with the shipped system services:
//! `ultar-init` (not PID 1) starts the log service, the bus, the settings
//! and the device service from the **shipped service files** (paths moved
//! below a work directory), each as its own user; the run checks who they
//! are, that output reaches the log service, that `init` answers on the
//! bus, and that the shutdown ends everything. Last line:
//! `INIT: ALLES OK` or `INIT: FEHLER <what>`.
//!
//! ```text
//! inittest --bin DIR --work DIR [--groups]
//! ```
//!
//! Runs as root (it switches ids). On Linux and on the Ultar kernel
//! (`tools/kern-test/init-e2e.sh`).

use std::os::fd::AsFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use ultar_bus::messages::init::{ServiceState, Status};
use ultar_bus::messages::log::{Query, SourceKind};
use ultar_bus::{Client, ClientOptions};

const SERVICES: [(&str, &str, u32); 4] = [
    (
        "logd",
        include_str!("../../../../rootfs/etc/ultar/services/logd.toml"),
        802,
    ),
    (
        "bus",
        include_str!("../../../../rootfs/etc/ultar/services/bus.toml"),
        801,
    ),
    (
        "settingsd",
        include_str!("../../../../rootfs/etc/ultar/services/settingsd.toml"),
        803,
    ),
    (
        "devd",
        include_str!("../../../../rootfs/etc/ultar/services/devd.toml"),
        804,
    ),
];
const POLICY: &str = include_str!("../../../../rootfs/etc/ultar/bus-policy.toml");
const WAIT: Duration = Duration::from_secs(90);

struct Run {
    bin: PathBuf,
    work: PathBuf,
}

impl Run {
    fn at(&self, path: &str) -> PathBuf {
        self.work.join(path)
    }

    /// A shipped service file with its paths below the work directory.
    fn moved(&self, text: &str) -> String {
        let work = self.work.display().to_string();
        text.replace("/run/ultar/", &format!("{work}/run/"))
            .replace("/data/ultar/", &format!("{work}/data/"))
            .replace(
                "/etc/ultar/bus-policy.toml",
                &format!("{work}/etc/bus-policy.toml"),
            )
            .replace("/usr/bin/", &format!("{}/", self.bin.display()))
            .replace("/usr/lib/ultar/drivers", &format!("{work}/depot"))
            .replace("/etc/ultar/trusted-keys", &format!("{work}/keys"))
    }

    fn layout(&self, groups: bool) -> std::io::Result<PathBuf> {
        let services = self.at("etc/services");
        std::fs::create_dir_all(&services)?;
        for (name, text, _) in SERVICES {
            std::fs::write(services.join(format!("{name}.toml")), self.moved(text))?;
        }
        std::fs::write(self.at("etc/bus-policy.toml"), POLICY)?;
        let config = self.at("etc/init.toml");
        let group_root = if groups {
            format!("group_root = \"{}\"\n", ultar_sys::machine::GROUP_ROOT)
        } else {
            String::new()
        };
        let text = format!(
            "shutdown_grace_ms = 5000\nservice_dirs = [\"{}\"]\noutput_socket = \"{}\"\n\
             bus_socket = \"{}\"\nbus_policy = \"{}\"\ncontrol_socket = \"{}\"\n\
             launcher = \"{}\"\n{group_root}",
            services.display(),
            self.at("run/log/init.sock").display(),
            self.at("run/bus/init.sock").display(),
            self.at("etc/bus-policy.toml").display(),
            self.at("ctl.sock").display(),
            self.bin.join("ultar-init").display(),
        );
        std::fs::write(&config, text)?;
        Ok(config)
    }

    fn admin(&self) -> Result<Client, String> {
        let options = ClientOptions {
            connect_retry_for: WAIT,
            ..ClientOptions::named("inittest")
        };
        Client::connect(&self.at("run/bus/system.sock"), options).map_err(|e| format!("bus {e}"))
    }
}

/// Waits until `init.status` reports all four services running.
fn wait_running(admin: &Client) -> Result<Vec<(String, u32)>, String> {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Ok(reply) = admin.call(&Status {}, Duration::from_secs(5)) {
            let running: Vec<(String, u32)> = reply
                .services
                .iter()
                .filter(|s| s.state == ServiceState::Running)
                .map(|s| (s.name.clone(), s.uid))
                .collect();
            if running.len() == SERVICES.len() {
                return Ok(running);
            }
        }
        if Instant::now() > deadline {
            return Err("services not running".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The user id of whoever listens on `socket` (its credentials at `listen`).
fn listener_uid(socket: &Path) -> Result<u32, String> {
    let stream = UnixStream::connect(socket).map_err(|e| format!("{}: {e}", socket.display()))?;
    ultar_sys::peer_credentials(stream.as_fd())
        .map(|cred| cred.uid)
        .map_err(|e| format!("peer credentials {e}"))
}

fn check_identities(run: &Run, running: &[(String, u32)]) -> Result<(), String> {
    for (name, _, uid) in SERVICES {
        if !running.contains(&(name.to_owned(), uid)) {
            return Err(format!("{name} not reported as uid {uid}"));
        }
    }
    for (socket, uid) in [("run/log/busd.sock", 802), ("run/bus/system.sock", 801)] {
        let found = listener_uid(&run.at(socket))?;
        if found != uid {
            return Err(format!("{socket}: listener uid {found}, expected {uid}"));
        }
    }
    for (dir, uid) in [
        ("data/log", 802),
        ("data/settings", 803),
        ("data/devd", 804),
    ] {
        let meta = std::fs::metadata(run.at(dir)).map_err(|e| format!("{dir}: {e}"))?;
        if meta.uid() != uid || meta.mode() & 0o777 != 0o700 {
            return Err(format!(
                "{dir}: owner {} mode {:o}",
                meta.uid(),
                meta.mode() & 0o777
            ));
        }
    }
    Ok(())
}

/// The log service holds the output lines init forwarded.
fn check_output(admin: &Client) -> Result<(), String> {
    let deadline = Instant::now() + WAIT;
    loop {
        let query = Query {
            service: Some("logd".into()),
            ..Query::default()
        };
        if let Ok(reply) = admin.call(&query, Duration::from_secs(5)) {
            if reply.records.iter().any(|r| r.source == SourceKind::Output) {
                return Ok(());
            }
        }
        if Instant::now() > deadline {
            return Err("no output of logd in the log".into());
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

fn stop(mut init: Child) -> Result<(), String> {
    let pid = nix::unistd::Pid::from_raw(init.id() as i32);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + WAIT;
    loop {
        match init.try_wait() {
            Ok(Some(status)) if status.code() == Some(0) => return Ok(()),
            Ok(Some(status)) => return Err(format!("init ended {status}")),
            Ok(None) if Instant::now() > deadline => return Err("init did not stop".into()),
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn test(run: &Run, groups: bool) -> Result<(), String> {
    let config = run.layout(groups).map_err(|e| format!("layout {e}"))?;
    let init = Command::new(run.bin.join("ultar-init"))
        .arg("--config")
        .arg(config)
        .spawn()
        .map_err(|e| format!("init {e}"))?;
    let checked = run.admin().and_then(|admin| {
        let running = wait_running(&admin)?;
        check_identities(run, &running)?;
        check_output(&admin)?;
        println!("INIT: services running as their users: {running:?}");
        Ok(())
    });
    let stopped = stop(init);
    checked.and(stopped)
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        let index = words.iter().position(|word| word == name)?;
        words.get(index + 1).map(PathBuf::from)
    };
    let (Some(bin), Some(work)) = (value("--bin"), value("--work")) else {
        println!("INIT: FEHLER usage: inittest --bin DIR --work DIR [--groups]");
        return;
    };
    let groups = words.iter().any(|word| word == "--groups");
    match test(&Run { bin, work }, groups) {
        Ok(()) => println!("INIT: ALLES OK"),
        Err(what) => println!("INIT: FEHLER {what}"),
    }
}
