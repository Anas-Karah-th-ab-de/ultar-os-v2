//! Setting up the Android end to end: the world, the services and the bus clients.

use super::*;

/// Where the test's files lie.
pub(super) struct Places {
    pub(super) foreign_root: PathBuf,
    pub(super) runtime: PathBuf,
    pub(super) downloads: PathBuf,
}

/// The test app (`package id label` of tools/foreign/android-e2e.sh).
pub(super) struct App {
    pub(super) package: String,
    pub(super) id: String,
    pub(super) label: String,
}

/// The display: Weston, its socket and the screenshot program.
pub(super) struct Display {
    pub(super) compositor: Child,
    pub(super) wayland: PathBuf,
    pub(super) shot: String,
    pub(super) shot_out: String,
}

/// The services of the test: bus, launcher, system dialog and the foreign service.
pub(super) struct Services {
    pub(super) output: Collector,
    pub(super) broker: Broker,
    pub(super) launcher: Arc<Launcher>,
    pub(super) life: Lifecycle,
    pub(super) serving: std::thread::JoinHandle<()>,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) authorize: Arc<AtomicBool>,
    pub(super) asked: Arc<Mutex<Vec<Text>>>,
    pub(super) foreign: Child,
}

/// Everything the end to end set up.
pub(super) struct Env {
    pub(super) world: World,
    pub(super) at: Places,
    pub(super) app: App,
    pub(super) display: Display,
    pub(super) run: Services,
    pub(super) files: Client,
    pub(super) settings: Client,
    pub(super) progress: ultar_bus::TypedSubscription<Progress>,
}

impl Env {
    pub(super) fn path(&self, name: &str) -> String {
        self.at.downloads.join(name).display().to_string()
    }

    /// Asks to install a file of Downloads; the job's number.
    pub(super) fn install(&self, name: &str) -> u64 {
        let install = Install {
            from: InstallFrom::File(self.path(name)),
            choices: Choices::default(),
        };
        started(call(&self.files, &install).outcome)
    }

    pub(super) fn end(&self, job: u64) -> Progress {
        end_of(&self.progress, job)
    }

    pub(super) fn inspect(&self, name: &str) -> Inspected {
        match call(
            &self.files,
            &Inspect {
                path: self.path(name),
            },
        )
        .outcome
        {
            InspectOutcome::Found(i) => i,
            other => panic!("{name}: {other:?}"),
        }
    }

    /// The app's folder in the area.
    pub(super) fn placed(&self) -> PathBuf {
        self.at.runtime.join(APPS).join(&self.app.package)
    }

    pub(super) fn home(&self) -> PathBuf {
        self.world.path(&format!("appdata/{}", self.app.id))
    }

    pub(super) fn asked(&self) -> usize {
        self.run.asked.lock().unwrap().len()
    }
}

/// The variables of tools/foreign/android-e2e.sh, when it runs this test as root.
pub(super) fn variables() -> Option<Vec<String>> {
    let names = [
        "ULTAR_FOREIGN_RUNTIME",
        "ULTAR_FOREIGN_APKS",
        "ULTAR_FOREIGN_APP",
        "ULTAR_FOREIGN_BIN",
        "ULTAR_FOREIGN_WESTON",
        "ULTAR_FOREIGN_SHOT",
        "ULTAR_FOREIGN_SHOT_OUT",
    ];
    let Ok(vars) = names
        .iter()
        .map(std::env::var)
        .collect::<Result<Vec<String>, _>>()
    else {
        eprintln!(
            "skipped: the ULTAR_FOREIGN_* variables of tools/foreign/android-e2e.sh are not set"
        );
        return None;
    };
    if !root() {
        eprintln!("skipped: real cells need root");
        return None;
    }
    Some(vars)
}

/// A copy of the area, the test apps in Downloads and a secret of the host.
pub(super) fn places(world: &World, area: &str, apks: &str) -> Places {
    let foreign_root = world.path("foreign");
    let runtime = foreign_root.join("linux");
    std::fs::create_dir_all(&foreign_root).unwrap();
    assert!(Command::new("cp")
        .args(["-a", area])
        .arg(&runtime)
        .status()
        .unwrap()
        .success());
    let downloads = world.path("areas/downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    for f in ["app", "arm", "tampered", "other", "old", "unsigned"] {
        std::fs::copy(
            Path::new(apks).join(format!("{f}.apk")),
            downloads.join(format!("{f}.apk")),
        )
        .unwrap();
    }
    std::fs::write(world.path("secret"), b"host secret").unwrap();
    Places {
        foreign_root,
        runtime,
        downloads,
    }
}

/// The probe in place of the Wayland bridge; its folder.
pub(super) fn probe_bridge(world: &World, wayland: &Path, package: &str) -> PathBuf {
    let bridge = world.path("bridge");
    std::fs::create_dir_all(&bridge).unwrap();
    let program = bridge.join("ultar-wayland");
    std::fs::write(&program, probe_script(world, wayland, package)).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    bridge
}

/// The foreign service under test, on the bus, with the copy of the area.
pub(super) fn foreign_service(world: &World, bin: &str, root: &Path) -> Child {
    Command::new(bin)
        .arg("--bus")
        .arg(socket(world, "system.foreign"))
        .arg("--root")
        .arg(root)
        .arg("--sources")
        .arg(world.path("areas"))
        .arg("--allow-root")
        .stdin(Stdio::null())
        .spawn()
        .unwrap()
}

/// The launcher with the area and the probe served on the bus, the system dialog and the
/// foreign service.
pub(super) fn services(
    world: &mut World,
    at: &Places,
    bridge: &Path,
    id: &str,
    bin: &str,
) -> Services {
    let tree = world.fake_group_tree();
    let dir = tree.join("apps").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    for file in ["cgroup.procs", "memory.max", "pids.max"] {
        std::fs::write(dir.join(file), b"").unwrap();
    }
    let output = Collector::listen(&world.path("out.sock"));
    let broker = broker(world);
    let config = format!(
        "{}foreign_runtime = \"{}\"\nforeign_bridge = \"{}\"\n",
        world.config_text(&[], Some(&tree)),
        at.runtime.display(),
        bridge.display()
    );
    let launcher = world.open(&config);
    let admins = Arc::new(Mutex::new(Admins::load(Some(&shipped_policy()))));
    let life = Lifecycle::new();
    let serving = {
        let (launcher, life) = (Arc::clone(&launcher), life.clone());
        let socket = socket(world, "system.launcher");
        std::thread::spawn(move || service::serve(socket, launcher, admins, life))
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (authorize, asked) = dialog(world, Arc::clone(&stop));
    let foreign = foreign_service(world, bin, &at.foreign_root);
    Services {
        output,
        broker,
        launcher,
        life,
        serving,
        stop,
        authorize,
        asked,
        foreign,
    }
}

pub(super) fn set_up(vars: &[String]) -> Env {
    let [area, apks, app, bin, weston_program, shot, shot_out] = vars else {
        unreachable!()
    };
    let mut parts = app.splitn(3, ' ').map(str::to_owned);
    let mut part = || parts.next().unwrap();
    let app = App {
        package: part(),
        id: part(),
        label: part(),
    };
    let mut world = World::new("android");
    let at = places(&world, area, apks);
    let (compositor, wayland) = weston(weston_program, &world.path("share/wl"));
    let bridge = probe_bridge(&world, &wayland, &app.package);
    let run = services(&mut world, &at, &bridge, &app.id, bin);
    let files = client(&world, "app.files");
    let settings = client(&world, "app.settings");
    let progress = files.subscribe::<Progress>().unwrap();
    assert!(common::wait_until_ok(|| settings
        .call(&RuntimeState {}, Duration::from_secs(1))
        .is_ok_and(|r| r.installed)));
    let (shot, shot_out) = (shot.clone(), shot_out.clone());
    let display = Display {
        compositor,
        wayland,
        shot,
        shot_out,
    };
    Env {
        world,
        at,
        app,
        display,
        run,
        files,
        settings,
        progress,
    }
}
