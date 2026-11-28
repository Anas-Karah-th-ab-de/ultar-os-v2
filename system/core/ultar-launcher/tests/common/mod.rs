//! A world for the process tests: keys, signed packages of the test app,
//! a configuration below a scratch directory, a launcher with its timer.
//!
//! As root the cells are real (namespaces, own users, a fake group tree
//! whose files the test reads); without root the operator's
//! `accept_missing` lets the same code run unconfined, which is what a
//! development machine and the CI (a user without root, umask 002) see.

#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use ultar_launcher::config::Config;
use ultar_launcher::platform::Platform;
use ultar_launcher::Launcher;
use ultar_service::log::Logger;

/// The launcher binary (cell stages, probe).
pub const HELPER: &str = env!("CARGO_BIN_EXE_ultar-launcher");
/// The test app.
pub const TESTAPP: &str = env!("CARGO_BIN_EXE_ultar-launcher-testapp");
/// `ulaunch`.
pub const ULAUNCH: &str = env!("CARGO_BIN_EXE_ulaunch");
/// Patience for anything a process does.
pub const WAIT: Duration = Duration::from_secs(20);

/// Polls `f` until it is true or [`WAIT`] passed.
pub fn wait_until_ok(mut f: impl FnMut() -> bool) -> bool {
    let end = std::time::Instant::now() + WAIT;
    while std::time::Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    f()
}

/// Whether the tests run with root (real cells).
pub fn root() -> bool {
    ultar_sys::euid() == 0
}

/// One scratch world.
pub struct World {
    pub dir: PathBuf,
    system_key: String,
    store_key: String,
    pub launcher: Option<Arc<Launcher>>,
    timer: Option<std::thread::JoinHandle<()>>,
}

impl World {
    /// A fresh world named `tag`.
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ulaunch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("keys/system")).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (system_key, system_pub) = ultar_pkg::keygen();
        let (store_key, store_pub) = ultar_pkg::keygen();
        std::fs::write(dir.join("keys/system/km.ed25519"), system_pub).unwrap();
        std::fs::write(dir.join("keys/store.ed25519"), store_pub).unwrap();
        Self {
            dir,
            system_key,
            store_key,
            launcher: None,
            timer: None,
        }
    }

    /// A path in the world.
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// The configuration text; `extra` is appended (overrides by position
    /// are not possible in TOML, so callers pass only new keys).
    pub fn config_text(&self, accept: &[&str], groups: Option<&Path>) -> String {
        let bin = Path::new(TESTAPP).parent().unwrap().display().to_string();
        let accept: Vec<String> = accept.iter().map(|a| format!("\"{a}\"")).collect();
        let group = groups.map_or(String::new(), |g| {
            format!("group_root = \"{}\"\n", g.display())
        });
        format!(
            "uid_first = 20000\nuid_last = 20009\n\
             system_view = [\"/usr\", \"/bin\", \"/lib\", \"/lib64\", \"{bin}\"]\nshares = [\"{d}/share\"]\n\
             accept_missing = [{}]\nmax_running = 8\nstop_grace_ms = 500\n\
             [paths]\nstate_dir = \"{d}/state\"\ndata_root = \"{d}/appdata\"\ncell_root = \"{d}/cells\"\n{group}\
             system_keys = \"{d}/keys/system\"\nstore_keys = \"{d}/keys\"\n\
             compositor_rights = \"{d}/share/compositor-apps.toml\"\noutput_socket = \"{d}/out.sock\"\n",
            accept.join(", "),
            d = self.dir.display()
        )
    }

    /// Opens the launcher with `config` and starts its timer.
    pub fn open(&mut self, config: &str) -> Arc<Launcher> {
        std::fs::create_dir_all(self.path("share")).unwrap();
        let config = Config::parse(config).unwrap();
        std::fs::create_dir_all(&config.paths.cell_root).unwrap();
        std::fs::set_permissions(
            &config.paths.cell_root,
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let platform = Platform::detect(
            Path::new(HELPER),
            &config.paths.cell_root,
            config.paths.group_root.as_deref(),
        );
        let launcher =
            Launcher::open(config, platform, PathBuf::from(HELPER), Logger::discard()).unwrap();
        let timer = {
            let launcher = Arc::clone(&launcher);
            std::thread::spawn(move || launcher.run_timer())
        };
        self.launcher = Some(Arc::clone(&launcher));
        self.timer = Some(timer);
        launcher
    }

    /// The configuration a test usually wants: real cells as root (with a
    /// fake group tree), everything accepted as missing without root.
    pub fn open_default(&mut self) -> Arc<Launcher> {
        let text = if root() {
            let tree = self.fake_group_tree();
            self.config_text(&[], Some(&tree))
        } else {
            self.config_text(&["ids", "cell", "groups", "net"], None)
        };
        self.open(&text)
    }

    /// A directory that looks like a group tree: the files of `apps/<id>`
    /// exist for the ids the tests use, so the limits can be read back.
    pub fn fake_group_tree(&self) -> PathBuf {
        let tree = self.path("groups");
        for app in [
            "good",
            "other",
            "evil",
            "svc",
            "crashy",
            "net",
            "store.good",
            "store.other",
            "store.evil",
            "store.net",
            "viewer",
            "single",
            "multi",
        ] {
            let dir = tree.join("apps").join(app);
            std::fs::create_dir_all(&dir).unwrap();
            for file in ["cgroup.procs", "memory.max", "pids.max"] {
                std::fs::write(dir.join(file), b"").unwrap();
            }
        }
        std::fs::write(tree.join("cgroup.procs"), b"").unwrap();
        tree
    }

    /// Stops the launcher and waits for its timer.
    pub fn close(&mut self) {
        if let Some(launcher) = self.launcher.take() {
            launcher.shutdown();
        }
        if let Some(timer) = self.timer.take() {
            timer.join().unwrap();
        }
    }

    /// Signs a package with manifest `text` as `<name>.uap`; `system`:
    /// with the system key (profile `app.system`), else the store key.
    pub fn sign(&self, name: &str, text: &str, system: bool) -> String {
        let key = if system {
            self.system_key.clone()
        } else {
            self.store_key.clone()
        };
        self.sign_with(name, text, &key)
    }

    /// Signs a package with manifest `text` with the private key `key`.
    pub fn sign_with(&self, name: &str, text: &str, key: &str) -> String {
        let src = self.path("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("readme"), b"test app").unwrap();
        let manifest = self.path(&format!("manifest-{name}.toml"));
        std::fs::write(&manifest, text).unwrap();
        let out = self.path(&format!("{name}.uap"));
        ultar_pkg::pack_and_sign(&manifest, &src, &out, key).unwrap();
        out.display().to_string()
    }

    /// Trusts one more key under `dir/name.ed25519` (`dir`: `keys` for the
    /// store, `keys/system`); returns its private key.
    pub fn another_key(&self, dir: &str, name: &str) -> String {
        let (private, public) = ultar_pkg::keygen();
        std::fs::write(self.path(&format!("{dir}/{name}.ed25519")), public).unwrap();
        private
    }

    /// The manifest text of a test-app package (`run`).
    pub fn manifest_text(id: &str, version: &str, extra: &str) -> String {
        format!(
            "id = \"{id}\"\nversion = \"{version}\"\nkind = \"native\"\nname = \"app.{id}.name\"\n\
             exec = \"{TESTAPP}\"\nargs = [\"run\"]\n{extra}"
        )
    }

    /// Leaves the launcher running without its timer's end or a shutdown:
    /// as if its process had been killed (its cells stay, their stage 1
    /// is a child of this test process).
    pub fn abandon(&mut self) {
        if let Some(launcher) = self.launcher.take() {
            std::mem::forget(launcher);
        }
        if let Some(timer) = self.timer.take() {
            std::mem::forget(timer);
        }
    }

    /// A package of the test app (`run`), with `extra` manifest lines.
    pub fn package(&self, id: &str, version: &str, extra: &str, system: bool) -> String {
        self.package_with(id, version, &["run"], extra, system)
    }

    /// A package whose window program runs the test app with `args`.
    pub fn package_args(&self, id: &str, args: &[&str], extra: &str, system: bool) -> String {
        self.package_with(id, "1.0.0", args, extra, system)
    }

    fn package_with(
        &self,
        id: &str,
        version: &str,
        args: &[&str],
        extra: &str,
        system: bool,
    ) -> String {
        let list: Vec<String> = args.iter().map(|a| format!("\"{a}\"")).collect();
        let text = format!(
            "id = \"{id}\"\nversion = \"{version}\"\nkind = \"native\"\nname = \"app.{id}.name\"\n\
             exec = \"{TESTAPP}\"\nargs = [{}]\n{extra}",
            list.join(", ")
        );
        self.sign(&format!("{id}-{version}"), &text, system)
    }
}

impl Drop for World {
    fn drop(&mut self) {
        self.close();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Lines `(service, text)` and the signal that one arrived.
type Lines = (std::sync::Mutex<Vec<(String, String)>>, std::sync::Condvar);

/// A stand-in for the log service's output channel: collects every line.
pub struct Collector {
    lines: Arc<Lines>,
}

impl Collector {
    /// Listens at `socket`.
    pub fn listen(socket: &Path) -> Self {
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        let lines: Arc<Lines> = Arc::default();
        let shared = Arc::clone(&lines);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || read_frames(stream, &shared));
            }
        });
        Self { lines }
    }

    /// Waits for a line of `service` that contains `text`; returns it.
    pub fn wait_for(&self, service: &str, text: &str) -> Option<String> {
        let deadline = std::time::Instant::now() + WAIT;
        let (lines, changed) = &*self.lines;
        let mut guard = lines.lock().unwrap();
        loop {
            if let Some((_, line)) = guard.iter().find(|(s, l)| s == service && l.contains(text)) {
                return Some(line.clone());
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return None;
            }
            guard = changed.wait_timeout(guard, left).unwrap().0;
        }
    }

    /// Every line of `service` so far.
    pub fn of(&self, service: &str) -> Vec<String> {
        let guard = self.lines.0.lock().unwrap();
        guard
            .iter()
            .filter(|(s, _)| s == service)
            .map(|(_, l)| l.clone())
            .collect()
    }
}

fn read_frames(mut stream: std::os::unix::net::UnixStream, shared: &Lines) {
    use std::io::Read;
    loop {
        let mut len = [0u8; 2];
        if stream.read_exact(&mut len).is_err() {
            return;
        }
        let mut body = vec![0u8; usize::from(u16::from_le_bytes(len))];
        if stream.read_exact(&mut body).is_err() {
            return;
        }
        let line = ultar_service::log::output::decode_body(&body).unwrap();
        shared.0.lock().unwrap().push((line.service, line.text));
        shared.1.notify_all();
    }
}

/// Waits for the instance of `app` to reach one of `states`.
pub fn wait_state(
    launcher: &Launcher,
    app: &str,
    role: ultar_bus::messages::launcher::Role,
    states: &[ultar_bus::messages::launcher::InstanceState],
) -> ultar_bus::messages::launcher::InstanceStatus {
    let status = launcher.wait_state(app, role, states, WAIT);
    let status = status.unwrap_or_else(|| panic!("{app}: no instance"));
    assert!(states.contains(&status.state), "{app}: {status:?}");
    status
}
