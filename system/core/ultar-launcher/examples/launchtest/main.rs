//! End-to-end run of the app launcher with the shipped definitions:
//! `ultar-init` (not PID 1) starts the log service, the bus and the
//! launcher from the **shipped service files** (paths moved below a work
//! directory); the run installs signed packages over the bus as the
//! administrator, starts apps in their cells and checks who they are, what
//! they see, that their output reaches the log service under their names,
//! that a malicious app stays in its cell (another app's data and process,
//! root, kernel rights, mounts, the system's files, privileged sockets,
//! memory and processes beyond its group's limits), stop, uninstall and
//! that a reinstalled app gets its user id back. Last line:
//! `LAUNCHER: ALLES OK` or `LAUNCHER: FEHLER <what>`.
//!
//! ```text
//! launchtest --bin DIR --work DIR [--groups]
//! ```
//!
//! Runs as root. On the Ultar kernel (`tools/kern-test/launcher-e2e.sh`,
//! `--groups`: the kernel's group tree) and on Linux.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use ultar_bus::messages::launcher::{AppStatus, Exit, Install, InstanceState, List, Role, Start};
use ultar_bus::messages::log::Query;
use ultar_bus::{Client, ClientOptions};

const SERVICES: [(&str, &str); 3] = [
    (
        "logd",
        include_str!("../../../../../rootfs/etc/ultar/services/logd.toml"),
    ),
    (
        "bus",
        include_str!("../../../../../rootfs/etc/ultar/services/bus.toml"),
    ),
    (
        "launcher",
        include_str!("../../../../../rootfs/etc/ultar/services/launcher.toml"),
    ),
];
const POLICY: &str = include_str!("../../../../../rootfs/etc/ultar/bus-policy.toml");
const CONFIG: &str = include_str!("../../../../../rootfs/etc/ultar/launcher.toml");
const WAIT: Duration = Duration::from_secs(120);
const CALL: Duration = Duration::from_secs(30);
const TESTAPP: &str = "ultar-launcher-testapp";

struct Run {
    bin: PathBuf,
    work: PathBuf,
    groups: bool,
}

impl Run {
    fn at(&self, path: &str) -> PathBuf {
        self.work.join(path)
    }

    fn moved(&self, text: &str) -> String {
        let work = self.work.display().to_string();
        text.replace("/run/ultar/", &format!("{work}/run/"))
            .replace("/data/ultar/", &format!("{work}/data/"))
            .replace(
                "/etc/ultar/bus-policy.toml",
                &format!("{work}/etc/bus-policy.toml"),
            )
            .replace(
                "/etc/ultar/launcher.toml",
                &format!("{work}/etc/launcher.toml"),
            )
            .replace("/etc/ultar/trusted-keys", &format!("{work}/keys"))
            .replace("/usr/bin/", &format!("{}/", self.bin.display()))
    }

    /// The launcher's configuration: the shipped one, moved, with the view
    /// of this machine and what the kernel is known to lack accepted.
    fn launcher_config(&self) -> String {
        let mut accept = Vec::new();
        if ultar_sys::ultar_rights().is_ok() {
            accept.push("\"net\""); // the Ultar kernel has no network namespaces yet
        }
        if !self.groups {
            accept.push("\"groups\"");
        }
        let view = format!(
            "system_view = [\"/bin\", \"/usr\", \"/lib\", \"/lib64\", \"{}\"]",
            self.bin.display()
        );
        self.moved(CONFIG)
            .lines()
            .filter(|line| self.groups || !line.starts_with("group_root"))
            .map(|line| match line {
                l if l.starts_with("system_view") => view.clone(),
                l if l.starts_with("shares") => "shares = []".to_owned(),
                l if l.starts_with("accept_missing") => {
                    format!("accept_missing = [{}]", accept.join(", "))
                }
                l => l.to_owned(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn layout(&self) -> std::io::Result<PathBuf> {
        let services = self.at("etc/services");
        std::fs::create_dir_all(&services)?;
        std::fs::create_dir_all(self.at("keys/system"))?;
        for (name, text) in SERVICES {
            let mut text = self.moved(text);
            if name == "launcher" && ultar_sys::ultar_rights().is_err() {
                // Linux: ids and namespaces need root (its rights list is not
                // enforced there); the Ultar kernel gives user 805 the rights
                text = text
                    .replace("user = 805\n", "")
                    .replace("\"--config\",", "\"--allow-root\", \"--config\",");
            }
            std::fs::write(services.join(format!("{name}.toml")), text)?;
        }
        std::fs::write(self.at("etc/bus-policy.toml"), POLICY)?;
        std::fs::write(self.at("etc/launcher.toml"), self.launcher_config())?;
        let group_root = if self.groups {
            format!("group_root = \"{}\"\n", ultar_sys::machine::GROUP_ROOT)
        } else {
            String::new()
        };
        let config = self.at("etc/init.toml");
        std::fs::write(
            &config,
            format!(
                "shutdown_grace_ms = 5000\nservice_dirs = [\"{}\"]\noutput_socket = \"{}\"\nbus_socket = \"{}\"\n\
                 bus_policy = \"{}\"\ncontrol_socket = \"{}\"\nlauncher = \"{}\"\n{group_root}",
                services.display(),
                self.at("run/log/init.sock").display(),
                self.at("run/bus/init.sock").display(),
                self.at("etc/bus-policy.toml").display(),
                self.at("ctl.sock").display(),
                self.bin.join("ultar-init").display(),
            ),
        )?;
        Ok(config)
    }
}

/// Signs a package of the test app.
struct Packages {
    system: String,
    store: String,
    run: PathBuf,
}

impl Packages {
    fn new(run: &Run) -> std::io::Result<Self> {
        let (system, system_pub) = ultar_pkg::keygen();
        let (store, store_pub) = ultar_pkg::keygen();
        std::fs::write(run.at("keys/system/km.ed25519"), system_pub)?;
        std::fs::write(run.at("keys/store.ed25519"), store_pub)?;
        std::fs::create_dir_all(run.at("src"))?;
        std::fs::write(run.at("src/readme"), "test app")?;
        Ok(Self {
            system,
            store,
            run: run.work.clone(),
        })
    }

    fn make(
        &self,
        bin: &Path,
        id: &str,
        args: &[&str],
        extra: &str,
        system: bool,
    ) -> Result<String, String> {
        let list: Vec<String> = args.iter().map(|a| format!("\"{a}\"")).collect();
        let text = format!(
            "id = \"{id}\"\nversion = \"1.0.0\"\nkind = \"native\"\nname = \"app.{id}.name\"\nexec = \"{}\"\nargs = [{}]\n{extra}",
            bin.join(TESTAPP).display(),
            list.join(", ")
        );
        let manifest = self.run.join(format!("{id}.toml"));
        let out = self.run.join(format!("{id}.uap"));
        std::fs::write(&manifest, text).map_err(|e| e.to_string())?;
        let key = if system { &self.system } else { &self.store };
        ultar_pkg::pack_and_sign(&manifest, &self.run.join("src"), &out, key)?;
        Ok(out.display().to_string())
    }
}

fn apps(admin: &Client) -> Result<Vec<AppStatus>, String> {
    admin
        .call(&List {}, CALL)
        .map(|r| r.apps)
        .map_err(|e| format!("list {e}"))
}

/// Waits until `app` is in one of `states`; returns its last end.
fn wait_app(admin: &Client, app: &str, states: &[InstanceState]) -> Result<Option<Exit>, String> {
    let deadline = Instant::now() + WAIT;
    loop {
        let now = apps(admin)?;
        if let Some(i) = now
            .iter()
            .find(|a| a.app == app)
            .and_then(|a| a.instances.first())
        {
            if states.contains(&i.state) {
                return Ok(i.last_exit);
            }
        }
        if Instant::now() > deadline {
            return Err(format!("{app} not in {states:?}: {now:?}"));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The line of `app`'s output in the log service that contains `text`.
fn output(admin: &Client, app: &str, text: &str) -> Result<String, String> {
    let deadline = Instant::now() + WAIT;
    loop {
        let query = Query {
            service: Some(format!("app-{}", app.replace('.', "_"))),
            ..Query::default()
        };
        if let Ok(reply) = admin.call(&query, CALL) {
            let lines: Vec<String> = reply.records.into_iter().filter_map(|r| r.text).collect();
            if let Some(line) = lines.iter().find(|l| l.contains(text)) {
                return Ok(line.clone());
            }
            if Instant::now() > deadline {
                return Err(format!("{app}: no line with {text:?} in {lines:?}"));
            }
        } else if Instant::now() > deadline {
            return Err(format!("{app}: log not readable"));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

fn install(admin: &Client, package: String) -> Result<u32, String> {
    admin
        .call(&Install { package }, CALL)
        .map(|r| r.uid)
        .map_err(|e| format!("install {e}"))
}

fn start(admin: &Client, app: &str) -> Result<u32, String> {
    let start = Start {
        app: app.into(),
        role: Role::Ui,
        activation_token: Some(5),
        uri: None,
        new_window: false,
    };
    admin.call(&start, CALL).map(|r| r.pid).map_err(|e| {
        let end = wait_app(
            admin,
            app,
            &[InstanceState::Crashed, InstanceState::Stopped],
        );
        format!("start {app} {e}: end {end:?}")
    })
}

mod checks;

fn stop_init(mut init: Child) -> Result<(), String> {
    ultar_sys::cell::send_signal(init.id(), ultar_sys::cell::StopSignal::Terminate)
        .map_err(|e| e.to_string())?;
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

fn test(run: &Run) -> Result<(), String> {
    let config = run.layout().map_err(|e| format!("layout {e}"))?;
    let packages = Packages::new(run).map_err(|e| format!("keys {e}"))?;
    let init = Command::new(run.bin.join("ultar-init"))
        .arg("--config")
        .arg(config)
        .spawn()
        .map_err(|e| format!("init {e}"))?;
    let options = ClientOptions {
        connect_retry_for: WAIT,
        ..ClientOptions::named("launchtest")
    };
    let checked = Client::connect(&run.at("run/bus/system.sock"), options)
        .map_err(|e| format!("bus {e}"))
        .and_then(|admin| checks::all(run, &packages, &admin));
    let stopped = stop_init(init);
    checked.and(stopped)
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        let index = words.iter().position(|word| word == name)?;
        words.get(index + 1).map(PathBuf::from)
    };
    let (Some(bin), Some(work)) = (value("--bin"), value("--work")) else {
        println!("LAUNCHER: FEHLER usage: launchtest --bin DIR --work DIR [--groups]");
        return;
    };
    let groups = words.iter().any(|word| word == "--groups");
    match test(&Run { bin, work, groups }) {
        Ok(()) => println!("LAUNCHER: ALLES OK"),
        Err(what) => println!("LAUNCHER: FEHLER {what}"),
    }
}
