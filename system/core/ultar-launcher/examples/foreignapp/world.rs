//! The device of `foreignapp`: bus, launcher, system dialog, compositor and the
//! service `ultar-foreign`, with the paths of the test machine.

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_bus::messages::foreign::{Progress, Step};
use ultar_bus::messages::launcher::AppStatus;
use ultar_bus::messages::sysdialog::{AuthResult, Authenticate, AuthenticateReply};
use ultar_bus::{Broker, Call, Client, ClientOptions, FixedIdentity, Identity, Policy};
use ultar_launcher::config::Config;
use ultar_launcher::platform::Platform;
use ultar_launcher::service::{self, Admins};
use ultar_launcher::Launcher;
use ultar_service::log::Logger;
use ultar_service::{Lifecycle, StopCause};

/// The app id `ultar-foreign` gives mousepad.
pub const APP: &str = "foreign.deb-mousepad";
const POLICY: &str = include_str!("../../../../../rootfs/etc/ultar/bus-policy.toml");
const WORK: &str = "/tmp/fa";
/// The compositor socket where the bridge looks for it (its default).
const UI_DIR: &str = "/run/ultar/ui";
const FOREIGN_ROOT: &str = "/data/foreign";
const SOURCES: &str = "/data/areas";
const IDENTITIES: [&str; 5] = [
    "system.launcher",
    "system.foreign",
    "system.sysdialog",
    "app.files",
    "app.settings",
];
const CALL: Duration = Duration::from_secs(60);

/// Command line.
pub struct Args {
    pub bin: PathBuf,
    pub deb: PathBuf,
    pub groups: bool,
    pub draw: u64,
}

impl Args {
    pub fn parse() -> Option<Self> {
        let words: Vec<String> = std::env::args().skip(1).collect();
        let value = |name: &str| {
            let i = words.iter().position(|w| w == name)?;
            words.get(i + 1).cloned()
        };
        Some(Self {
            bin: PathBuf::from(value("--bin")?),
            deb: PathBuf::from(value("--deb")?),
            groups: words.iter().any(|w| w == "--groups"),
            draw: value("--draw").map_or(Some(60), |d| d.parse().ok())?,
        })
    }
}

/// Polls `f` until it is true or `limit` passed.
pub fn poll(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    f()
}

/// The end of `job` on `foreign.progress`.
pub fn end_of(
    progress: &ultar_bus::TypedSubscription<Progress>,
    job: u64,
    limit: Duration,
) -> Option<Progress> {
    let end = Instant::now() + limit;
    loop {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return None;
        }
        let Ok(p) = progress.recv_timeout(left) else {
            continue;
        };
        if p.job == job && matches!(p.step, Step::Done | Step::Cancelled | Step::Failed(_)) {
            return Some(p);
        }
    }
}

/// The running device.
pub struct World {
    pub deb: PathBuf,
    pub launcher: Arc<Launcher>,
    groups: bool,
    _broker: Broker,
    life: Lifecycle,
    stop_dialog: Arc<AtomicBool>,
    asked: Arc<AtomicUsize>,
    children: Mutex<Vec<Child>>,
}

fn socket(identity: &str) -> PathBuf {
    Path::new(WORK).join(format!("bus/{identity}.sock"))
}

fn dir(path: &Path, mode: u32) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

fn config_text(groups: bool) -> String {
    let group = if groups {
        format!("group_root = \"{}\"\n", ultar_sys::machine::GROUP_ROOT)
    } else {
        String::new()
    };
    let accept = if groups { "" } else { "\"groups\"" };
    format!(
        "uid_first = 20000\nuid_last = 20009\nsystem_view = [\"/bin\"]\nshares = [\"{UI_DIR}\"]\n\
         accept_missing = [{accept}]\nmax_running = 4\nstop_grace_ms = 3000\n\
         [paths]\nstate_dir = \"{WORK}/state\"\ndata_root = \"{WORK}/appdata\"\ncell_root = \"{WORK}/cells\"\n{group}\
         system_keys = \"{WORK}/keys/system\"\nstore_keys = \"{WORK}/keys\"\n\
         compositor_rights = \"{WORK}/share/compositor-apps.toml\"\noutput_socket = \"{WORK}/out.sock\"\n\
         foreign_runtime = \"{FOREIGN_ROOT}/linux\"\nforeign_bridge = \"{WORK}/bridge\"\n"
    )
}

fn layout(bin: &Path) -> std::io::Result<()> {
    for (path, mode) in [
        ("", 0o755),
        ("keys/system", 0o755),
        ("state", 0o700),
        ("appdata", 0o700),
        ("cells", 0o700),
        ("share", 0o755),
        ("bridge", 0o755),
        ("bus", 0o755),
    ] {
        dir(&Path::new(WORK).join(path), mode)?;
    }
    dir(Path::new(UI_DIR), 0o755)?;
    dir(Path::new(SOURCES), 0o755)?;
    let (_, system) = ultar_pkg::keygen();
    let (_, store) = ultar_pkg::keygen();
    std::fs::write(format!("{WORK}/keys/system/km.ed25519"), system)?;
    std::fs::write(format!("{WORK}/keys/store.ed25519"), store)?;
    std::fs::write(format!("{WORK}/policy.toml"), POLICY)?;
    let bridge = Path::new(WORK).join("bridge/ultar-wayland");
    std::fs::copy(bin.join("ultar-wayland"), &bridge)?;
    std::fs::set_permissions(&bridge, std::fs::Permissions::from_mode(0o755))
}

fn broker() -> Result<Broker, String> {
    let policy =
        Policy::load(Path::new(&format!("{WORK}/policy.toml"))).map_err(|e| format!("{e:?}"))?;
    let mut builder = Broker::builder(policy);
    for identity in IDENTITIES {
        let fixed = Arc::new(FixedIdentity::new(
            Identity::new(identity).map_err(|e| format!("{e:?}"))?,
        ));
        builder = builder.listen(&socket(identity), fixed);
    }
    builder.start().map_err(|e| format!("{e:?}"))
}

fn open_launcher(bin: &Path, groups: bool) -> Result<Arc<Launcher>, String> {
    let config = Config::parse(&config_text(groups)).map_err(|e| format!("config {e:?}"))?;
    let helper = bin.join("ultar-launcher");
    let platform = Platform::detect(
        &helper,
        &config.paths.cell_root,
        config.paths.group_root.as_deref(),
    );
    let launcher = Launcher::open(config, platform, helper, Logger::discard())
        .map_err(|e| format!("launcher {e:?}"))?;
    let timer = Arc::clone(&launcher);
    std::thread::spawn(move || timer.run_timer());
    Ok(launcher)
}

/// The system dialog: every question is answered "authorized" and counted.
fn dialog(stop: Arc<AtomicBool>, asked: Arc<AtomicUsize>) -> Result<(), String> {
    let client = Client::connect(
        &socket("system.sysdialog"),
        ClientOptions::named("system.sysdialog"),
    )
    .map_err(|e| format!("sysdialog {e:?}"))?;
    let endpoint = client
        .register("sysdialog")
        .map_err(|e| format!("sysdialog {e:?}"))?;
    std::thread::spawn(move || {
        let _keep = client;
        while !stop.load(Ordering::SeqCst) {
            let Ok(request) = endpoint.recv_timeout(Duration::from_millis(100)) else {
                continue;
            };
            if request.decode::<Authenticate>().is_ok() {
                asked.fetch_add(1, Ordering::SeqCst);
                let _ = request.reply(&AuthenticateReply {
                    result: AuthResult::Authorized,
                });
            }
        }
    });
    Ok(())
}

/// Fork + exec (the Ultar kernel releases a vfork parent only when the child exits).
fn spawn(program: &Path, args: &[&str]) -> std::io::Result<Child> {
    use std::os::unix::process::CommandExt;
    Command::new(program)
        .args(args)
        .uid(0)
        .stdin(Stdio::null())
        .spawn()
}

impl World {
    pub fn start(args: &Args) -> Result<Self, String> {
        layout(&args.bin).map_err(|e| format!("layout {e}"))?;
        let deb = Path::new(SOURCES).join("mousepad.deb");
        std::fs::copy(&args.deb, &deb).map_err(|e| format!("deb {e}"))?;
        let broker = broker()?;
        let launcher = open_launcher(&args.bin, args.groups)?;
        let life = Lifecycle::new();
        let admins = Arc::new(Mutex::new(Admins::load(Some(Path::new(&format!(
            "{WORK}/policy.toml"
        ))))));
        {
            let (launcher, life) = (Arc::clone(&launcher), life.clone());
            std::thread::spawn(move || {
                service::serve(socket("system.launcher"), launcher, admins, life)
            });
        }
        let (stop_dialog, asked) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicUsize::new(0)),
        );
        dialog(Arc::clone(&stop_dialog), Arc::clone(&asked))?;
        let rights = format!("{WORK}/share/compositor-apps.toml");
        let socket_path = format!("{UI_DIR}/compositor.sock");
        let compositor = spawn(
            &args.bin.join("ultar-compositor"),
            &[
                "--socket",
                &socket_path,
                "--allow-root",
                "--lang",
                "ar",
                "--no-cursor",
                "--app-rights",
                &rights,
                "--app-uids",
                "20000-20009",
                "--app-rights-owner",
                "0",
            ],
        )
        .map_err(|e| format!("compositor {e}"))?;
        if !poll(Duration::from_secs(120), || {
            Path::new(&socket_path).exists()
        }) {
            return Err("the compositor has no socket".into());
        }
        let bus = socket("system.foreign").display().to_string();
        let service = spawn(
            &args.bin.join("ultar-foreign"),
            &[
                "--bus",
                &bus,
                "--root",
                FOREIGN_ROOT,
                "--sources",
                SOURCES,
                "--allow-root",
            ],
        )
        .map_err(|e| format!("ultar-foreign {e}"))?;
        Ok(Self {
            deb,
            launcher,
            groups: args.groups,
            _broker: broker,
            life,
            stop_dialog,
            asked,
            children: Mutex::new(vec![compositor, service]),
        })
    }

    pub fn client(&self, identity: &str) -> Client {
        Client::connect(&socket(identity), ClientOptions::named(identity)).unwrap()
    }

    pub fn call<C: Call>(&self, client: &Client, message: &C) -> C::Reply {
        client.call(message, CALL).unwrap()
    }

    pub fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }

    pub fn app(&self) -> Option<AppStatus> {
        self.launcher.list().into_iter().find(|a| a.app == APP)
    }

    pub fn compositor_rights(&self) -> PathBuf {
        Path::new(WORK).join("share/compositor-apps.toml")
    }

    pub fn app_data(&self) -> PathBuf {
        Path::new(WORK).join("appdata").join(APP)
    }

    /// The processes of the app's cell (its memory group, or by name without groups).
    pub fn cell_pids(&self) -> Vec<u32> {
        // the app's group and its programs by name (a kernel whose group file stays empty)
        let mut pids = named(&["mousepad", "ultar-wayland"]);
        if self.groups {
            let procs = Path::new(ultar_sys::machine::GROUP_ROOT)
                .join("apps")
                .join(APP)
                .join("cgroup.procs");
            pids.extend(
                std::fs::read_to_string(procs)
                    .unwrap_or_default()
                    .lines()
                    .filter_map(|l| l.trim().parse::<u32>().ok()),
            );
        }
        pids.sort_unstable();
        pids.dedup();
        pids
    }

    /// The program itself runs in the cell (not only the bridge).
    pub fn window_drawn(&self) -> bool {
        self.cell_pids().iter().any(|pid| comm(*pid) == "mousepad")
    }

    /// Files below the app's data owned by `uid` (what the program wrote as itself).
    pub fn data_owned_by(&self, uid: u32) -> usize {
        fn walk(dir: &Path, uid: u32, n: &mut usize) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let Ok(meta) = entry.path().symlink_metadata() else {
                    continue;
                };
                if meta.uid() == uid {
                    *n += 1;
                }
                if meta.is_dir() {
                    walk(&entry.path(), uid, n);
                }
            }
        }
        let mut n = 0;
        walk(&self.app_data(), uid, &mut n);
        n
    }

    pub fn installed(&self, package: &str) -> bool {
        ["", ":amd64", ":all"].iter().any(|arch| {
            Path::new(FOREIGN_ROOT)
                .join(format!("linux/var/lib/dpkg/info/{package}{arch}.list"))
                .exists()
        })
    }

    pub fn stop(&self) {
        self.stop_dialog.store(true, Ordering::SeqCst);
        self.life.request_stop(StopCause::Request);
        for child in self.children.lock().unwrap().iter_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn comm(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn named(names: &[&str]) -> Vec<u32> {
    std::fs::read_dir("/proc")
        .map(|d| {
            d.flatten()
                .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
                .filter(|pid| names.contains(&comm(*pid).as_str()))
                .collect()
        })
        .unwrap_or_default()
}
