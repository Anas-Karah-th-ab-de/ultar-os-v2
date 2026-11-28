//! What the end-to-end tests of apps from other systems share (`foreign_e2e.rs`,
//! `foreign_windows_e2e.rs`): the bus with the shipped policy, the launcher's service on
//! it, a system dialog that answers as told, the real service `ultar-foreign` and the
//! test's stand-in for the Wayland bridge.

#![allow(dead_code)]

use crate::common::{World, WAIT};
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use ultar_bus::messages::foreign::{InstallOutcome, Progress, Step};
use ultar_bus::messages::sysdialog::{AuthResult, Authenticate, AuthenticateReply};
use ultar_bus::{Broker, Call, Client, ClientOptions, FixedIdentity, Identity, Policy, Text};
use ultar_launcher::service::{self, Admins};
use ultar_launcher::Launcher;
use ultar_service::{Lifecycle, StopCause};

pub fn shipped_policy() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../rootfs/etc/ultar/bus-policy.toml")
}

pub fn socket(world: &World, identity: &str) -> PathBuf {
    world.path(&format!("{identity}.sock"))
}

/// The broker with the shipped policy, one socket per identity.
fn broker(world: &World, identities: &[&str]) -> Broker {
    let mut builder = Broker::builder(Policy::load(&shipped_policy()).unwrap());
    for identity in identities {
        let fixed = Arc::new(FixedIdentity::new(Identity::new(identity).unwrap()));
        builder = builder.listen(&socket(world, identity), fixed);
    }
    builder.start().unwrap()
}

pub fn client(world: &World, identity: &str) -> Client {
    Client::connect(&socket(world, identity), ClientOptions::named(identity)).unwrap()
}

pub fn call<C: Call>(client: &Client, message: &C) -> C::Reply {
    client.call(message, WAIT).unwrap()
}

/// The system dialog: answers every question with `authorize` and keeps the questions.
pub struct Dialog {
    pub authorize: Arc<AtomicBool>,
    pub asked: Arc<Mutex<Vec<Text>>>,
}

fn dialog(world: &World, stop: Arc<AtomicBool>) -> Dialog {
    let client = client(world, "system.sysdialog");
    let endpoint = client.register("sysdialog").unwrap();
    let authorize = Arc::new(AtomicBool::new(false));
    let asked: Arc<Mutex<Vec<Text>>> = Arc::default();
    let (a, q) = (Arc::clone(&authorize), Arc::clone(&asked));
    std::thread::spawn(move || {
        let _keep = client;
        while !stop.load(Ordering::SeqCst) {
            let Ok(request) = endpoint.recv_timeout(Duration::from_millis(50)) else {
                continue;
            };
            let Ok(question) = request.decode::<Authenticate>() else {
                continue;
            };
            assert!(
                ["app.files", "app.settings"].contains(&question.app_id.as_str()),
                "the caller is named in the dialog: {}",
                question.app_id
            );
            q.lock().unwrap().push(question.action);
            let result = if a.load(Ordering::SeqCst) {
                AuthResult::Authorized
            } else {
                AuthResult::Cancelled
            };
            let _ = request.reply(&AuthenticateReply { result });
        }
    });
    Dialog { authorize, asked }
}

/// The running device: the bus, the launcher serving on it, the dialog and the service.
pub struct Device {
    broker: Broker,
    pub launcher: Arc<Launcher>,
    life: Lifecycle,
    serving: JoinHandle<()>,
    stop_dialog: Arc<AtomicBool>,
    pub dialog: Dialog,
    service: Child,
}

impl Device {
    /// Starts everything on the sockets of `identities`: the launcher with `config`, the
    /// service `bin` with its root `foreign_root` and the person's areas as sources.
    pub fn start(
        world: &mut World,
        identities: &[&str],
        config: &str,
        bin: &str,
        foreign_root: &Path,
    ) -> Self {
        let broker = broker(world, identities);
        let launcher = world.open(config);
        let admins = Arc::new(Mutex::new(Admins::load(Some(&shipped_policy()))));
        let life = Lifecycle::new();
        let serving = {
            let (launcher, life, socket) = (
                Arc::clone(&launcher),
                life.clone(),
                socket(world, "system.launcher"),
            );
            std::thread::spawn(move || service::serve(socket, launcher, admins, life))
        };
        let stop_dialog = Arc::new(AtomicBool::new(false));
        let dialog = dialog(world, Arc::clone(&stop_dialog));
        let service = Command::new(bin)
            .arg("--bus")
            .arg(socket(world, "system.foreign"))
            .arg("--root")
            .arg(foreign_root)
            .arg("--sources")
            .arg(world.path("areas"))
            .arg("--allow-root")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        Self {
            broker,
            launcher,
            life,
            serving,
            stop_dialog,
            dialog,
            service,
        }
    }

    /// Lets the dialog answer `authorize` from now on.
    pub fn authorize(&self, authorize: bool) {
        self.dialog.authorize.store(authorize, Ordering::SeqCst);
    }

    /// Ends the service (it must end cleanly), the dialog, the launcher's service and the bus.
    pub fn stop(self) {
        let Self {
            broker,
            launcher: _launcher,
            life,
            serving,
            stop_dialog,
            dialog: _dialog,
            mut service,
        } = self;
        let _ = Command::new("kill")
            .args(["-TERM", &service.id().to_string()])
            .status();
        assert_eq!(service.wait().unwrap().code(), Some(0));
        stop_dialog.store(true, Ordering::SeqCst);
        life.request_stop(StopCause::Request);
        drop(broker);
        serving.join().unwrap();
    }
}

/// Waits (at most `limit`) for the end of `job` on `foreign.progress`.
pub fn end_of(
    progress: &ultar_bus::TypedSubscription<Progress>,
    job: u64,
    limit: Duration,
) -> Progress {
    let deadline = Instant::now() + limit;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "job {job} did not end");
        let Ok(p) = progress.recv_timeout(left) else {
            continue;
        };
        if p.job == job && matches!(p.step, Step::Done | Step::Cancelled | Step::Failed(_)) {
            return p;
        }
    }
}

pub fn started(outcome: InstallOutcome) -> u64 {
    match outcome {
        InstallOutcome::Started(job) => job,
        other => panic!("{other:?}"),
    }
}

/// The packages dpkg knows in the area `runtime`.
pub fn dpkg_info(runtime: &Path) -> BTreeSet<String> {
    std::fs::read_dir(runtime.join("var/lib/dpkg/info"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

/// Whether dpkg has `package` in the area `runtime`.
pub fn installed(runtime: &Path, package: &str) -> bool {
    ["", ":amd64", ":arm64", ":all"].iter().any(|arch| {
        runtime
            .join(format!("var/lib/dpkg/info/{package}{arch}.list"))
            .exists()
    })
}

/// A copy of the Linux area `area` at `runtime` (the original stays as it is).
pub fn copy_area(area: &str, runtime: &Path) {
    std::fs::create_dir_all(runtime.parent().unwrap()).unwrap();
    let copied = Command::new("cp")
        .args(["-a", area])
        .arg(runtime)
        .status()
        .unwrap();
    assert!(copied.success());
}

/// The test's stand-in for the Wayland bridge: `bridge/ultar-wayland` running `script`;
/// returns its directory.
pub fn fake_bridge(world: &World, script: &str) -> PathBuf {
    let bridge = world.path("bridge");
    std::fs::create_dir_all(&bridge).unwrap();
    std::fs::write(bridge.join("ultar-wayland"), script).unwrap();
    std::fs::set_permissions(
        bridge.join("ultar-wayland"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    bridge
}

/// The fake group tree has no controller that makes an app's group files.
pub fn group_files(tree: &Path, app: &str) {
    let dir = tree.join("apps").join(app);
    std::fs::create_dir_all(&dir).unwrap();
    for file in ["cgroup.procs", "memory.max", "pids.max"] {
        std::fs::write(dir.join(file), b"").unwrap();
    }
}

/// The value after `key` in the probe's report.
pub fn line<'a>(probe: &'a str, key: &str) -> &'a str {
    probe
        .lines()
        .find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix(' ')))
        .unwrap_or_else(|| panic!("{key} missing in\n{probe}"))
}

/// The text of `path` once it contains `text` (at most `limit`).
pub fn wait_file(path: &Path, text: &str, limit: Duration) -> String {
    let deadline = Instant::now() + limit;
    loop {
        if let Ok(t) = std::fs::read_to_string(path) {
            if t.contains(text) {
                return t;
            }
        }
        assert!(
            Instant::now() < deadline,
            "{} has no {text}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A path as the service's messages carry it.
pub fn text(path: &Path) -> String {
    path.display().to_string()
}
