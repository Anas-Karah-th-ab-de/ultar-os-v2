//! End to end on Linux (root, real cells): an Android app (APK) of the person through
//! `ultar-foreign` and the launcher, with the shipped bus policy, started by the Android
//! runtime of the Linux area (ATL) and shown on a headless Weston.
//!
//! `tools/foreign/android-e2e.sh` sets:
//!
//! * `ULTAR_FOREIGN_RUNTIME` – the area with the component "Android apps"
//!   (`tools/foreign/make-android-runtime.sh`; copied here, the original stays)
//! * `ULTAR_FOREIGN_APKS` – a directory with `app.apk` (the test app), and its variants
//!   `arm.apk` (only ARM code), `tampered.apk` (one byte changed), `other.apk` (the same
//!   package from another signer), `old.apk` (JAR signing only), `unsigned.apk`
//! * `ULTAR_FOREIGN_APP` – `<package> <id> <label>` of the test app
//! * `ULTAR_FOREIGN_BIN` – `target/…/ultar-foreign`
//! * `ULTAR_FOREIGN_WESTON` – the `weston` program (headless, pixman, debug)
//! * `ULTAR_FOREIGN_SHOT` – `shot.sh SOCKET PNG`: takes a screenshot, says whether it shows
//!   more than an empty desktop (exit 0)
//! * `ULTAR_FOREIGN_SHOT_OUT` – where the screenshot is kept
//!
//! Without them (or without root) the test says so and passes.

mod common;

use common::{root, Collector, World, WAIT};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_bus::messages::foreign::{
    Choices, Failure, Inspect, InspectOutcome, Install, InstallFrom, InstallOutcome, List, Outcome,
    Progress, Refusal, Remove, RuntimeState, Source, Step,
};
use ultar_bus::messages::launcher::Role;
use ultar_bus::messages::sysdialog::{AuthResult, Authenticate, AuthenticateReply};
use ultar_bus::{Broker, Call, Client, ClientOptions, FixedIdentity, Identity, Policy, Text};
use ultar_launcher::service::{self, Admins};
use ultar_service::{Lifecycle, StopCause};

const JOB_WAIT: Duration = Duration::from_secs(600);
/// ATL's first start (ART, the app's classes) is slow on a software renderer.
const WINDOW_WAIT: Duration = Duration::from_secs(300);
/// Where the area keeps the Android apps.
const APPS: &str = "opt/ultar-android/apps";

fn shipped_policy() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../rootfs/etc/ultar/bus-policy.toml")
}

const IDENTITIES: [&str; 5] = [
    "system.launcher",
    "system.foreign",
    "system.sysdialog",
    "app.files",
    "app.settings",
];

fn socket(world: &World, identity: &str) -> PathBuf {
    world.path(&format!("{identity}.sock"))
}

fn broker(world: &World) -> Broker {
    let mut builder = Broker::builder(Policy::load(&shipped_policy()).unwrap());
    for identity in IDENTITIES {
        let fixed = Arc::new(FixedIdentity::new(Identity::new(identity).unwrap()));
        builder = builder.listen(&socket(world, identity), fixed);
    }
    builder.start().unwrap()
}

fn client(world: &World, identity: &str) -> Client {
    Client::connect(&socket(world, identity), ClientOptions::named(identity)).unwrap()
}

fn call<C: Call>(client: &Client, message: &C) -> C::Reply {
    client.call(message, WAIT).unwrap()
}

fn dialog(world: &World, stop: Arc<AtomicBool>) -> (Arc<AtomicBool>, Arc<Mutex<Vec<Text>>>) {
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
            q.lock().unwrap().push(question.action);
            let result = if a.load(Ordering::SeqCst) {
                AuthResult::Authorized
            } else {
                AuthResult::Cancelled
            };
            let _ = request.reply(&AuthenticateReply { result });
        }
    });
    (authorize, asked)
}

fn end_of(progress: &ultar_bus::TypedSubscription<Progress>, job: u64) -> Progress {
    let deadline = Instant::now() + JOB_WAIT;
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

fn started(outcome: InstallOutcome) -> u64 {
    match outcome {
        InstallOutcome::Started(job) => job,
        other => panic!("{other:?}"),
    }
}

fn sha256(path: &Path) -> String {
    let out = Command::new("sha256sum").arg(path).output().unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// The stand-in for the Wayland bridge: looks around the cell, then runs the app with the
/// test's headless Weston as its display.
fn probe_script(world: &World, wayland: &Path, package: &str) -> String {
    format!(
        r#"#!/bin/sh
out="$HOME/probe.txt"
: >"$out"
r() {{ echo "$1 $2" >>"$out"; }}
r args "$*"
r uid "$(id -u)"
for p in /usr/bin/ultar-probe /{APPS}/{package}/ultar-probe /{APPS}/{package}/lib/ultar-probe /opt/ultar-android/bin/ultar-probe; do
  if touch "$p" 2>/dev/null; then r write-area "FAIL:$p"; fi
done
r write-area done
if cat "{secret}" >/dev/null 2>&1; then r host-secret FAIL; else r host-secret hidden; fi
r net "$(tail -n +3 /proc/net/dev | cut -d: -f1 | tr -d ' ' | tr '\n' ' ')"
r caps "$(grep CapEff /proc/self/status | tr -s '\t ' ' ' | cut -d' ' -f2)"
r nnp "$(grep NoNewPrivs /proc/self/status | tr -s '\t ' ' ' | cut -d' ' -f2)"
r devices "$(find /dev \( -type c -o -type b \) -printf '%f ' 2>/dev/null)"
r id-files "$(ls /etc/machine-id /var/lib/dbus/machine-id 2>/dev/null | while read -r f; do printf '%s=%s ' "$f" "$(wc -c <"$f")"; done)"
if printf '#!/bin/sh\necho x\n' >"$HOME/exec-probe" && chmod +x "$HOME/exec-probe" && "$HOME/exec-probe" >/dev/null 2>&1; then r home-exec FAIL; else r home-exec refused; fi
rm -f "$HOME/exec-probe"
r end ok
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
export WAYLAND_DISPLAY="{wayland}" GSK_RENDERER=cairo LIBGL_ALWAYS_SOFTWARE=1
shift
exec "$@" >"$HOME/app.log" 2>&1
"#,
        secret = world.path("secret").display(),
        wayland = wayland.display(),
    )
}

fn line<'a>(probe: &'a str, key: &str) -> &'a str {
    probe
        .lines()
        .find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix(' ')))
        .unwrap_or_else(|| panic!("{key} missing in\n{probe}"))
}

fn wait_file(path: &Path, text: &str) -> String {
    let deadline = Instant::now() + WAIT;
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

/// A headless Weston whose socket the app's cell reaches (the test's shared directory).
fn weston(program: &str, dir: &Path) -> (Child, PathBuf) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let child = Command::new(program)
        .args([
            "--backend=headless",
            "--renderer=pixman",
            "--width=1024",
            "--height=768",
            "--socket=wayland-e2e",
            "--idle-time=0",
            "--debug",
        ])
        .env("XDG_RUNTIME_DIR", dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(
            std::fs::File::create(dir.with_extension("log")).unwrap(),
        ))
        .spawn()
        .unwrap();
    let sock = dir.join("wayland-e2e");
    assert!(
        common::wait_until_ok(|| sock.exists()),
        "weston did not start (see {})",
        dir.with_extension("log").display()
    );
    // the app runs as its own user: it may use the socket, nothing else in the directory
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o711)).unwrap();
    std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o666)).unwrap();
    (child, sock)
}

#[test]
fn an_android_app_goes_in_shows_its_window_in_its_cell_and_leaves_without_trace() {
    let vars: Vec<Result<String, _>> = [
        "ULTAR_FOREIGN_RUNTIME",
        "ULTAR_FOREIGN_APKS",
        "ULTAR_FOREIGN_APP",
        "ULTAR_FOREIGN_BIN",
        "ULTAR_FOREIGN_WESTON",
        "ULTAR_FOREIGN_SHOT",
        "ULTAR_FOREIGN_SHOT_OUT",
    ]
    .iter()
    .map(std::env::var)
    .collect();
    let Ok(vars) = vars.into_iter().collect::<Result<Vec<String>, _>>() else {
        eprintln!(
            "skipped: the ULTAR_FOREIGN_* variables of tools/foreign/android-e2e.sh are not set"
        );
        return;
    };
    if !root() {
        eprintln!("skipped: real cells need root");
        return;
    }
    let [area, apks, app, bin, weston_program, shot, shot_out] = vars.as_slice() else {
        unreachable!()
    };
    let mut parts = app.splitn(3, ' ');
    let (package, id, label) = (
        parts.next().unwrap().to_owned(),
        parts.next().unwrap().to_owned(),
        parts.next().unwrap().to_owned(),
    );
    let mut world = World::new("android");
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
    let (mut compositor, wayland) = weston(weston_program, &world.path("share/wl"));
    let bridge = world.path("bridge");
    std::fs::create_dir_all(&bridge).unwrap();
    std::fs::write(
        bridge.join("ultar-wayland"),
        probe_script(&world, &wayland, &package),
    )
    .unwrap();
    std::fs::set_permissions(
        bridge.join("ultar-wayland"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let tree = world.fake_group_tree();
    let dir = tree.join("apps").join(&id);
    std::fs::create_dir_all(&dir).unwrap();
    for file in ["cgroup.procs", "memory.max", "pids.max"] {
        std::fs::write(dir.join(file), b"").unwrap();
    }
    let output = Collector::listen(&world.path("out.sock"));

    let _broker = broker(&world);
    let config = format!(
        "{}foreign_runtime = \"{}\"\nforeign_bridge = \"{}\"\n",
        world.config_text(&[], Some(&tree)),
        runtime.display(),
        bridge.display()
    );
    let launcher = world.open(&config);
    let admins = Arc::new(Mutex::new(Admins::load(Some(&shipped_policy()))));
    let life = Lifecycle::new();
    let serving = {
        let (launcher, life, socket) = (
            Arc::clone(&launcher),
            life.clone(),
            socket(&world, "system.launcher"),
        );
        std::thread::spawn(move || service::serve(socket, launcher, admins, life))
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (authorize, asked) = dialog(&world, Arc::clone(&stop));
    let mut service = Command::new(bin)
        .arg("--bus")
        .arg(socket(&world, "system.foreign"))
        .arg("--root")
        .arg(&foreign_root)
        .arg("--sources")
        .arg(world.path("areas"))
        .arg("--allow-root")
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let files = client(&world, "app.files");
    let settings = client(&world, "app.settings");
    let progress = files.subscribe::<Progress>().unwrap();
    assert!(common::wait_until_ok(|| settings
        .call(&RuntimeState {}, Duration::from_secs(1))
        .is_ok_and(|r| r.installed)));
    let path = |name: &str| downloads.join(name).display().to_string();
    let install = |name: &str| Install {
        from: InstallFrom::File(path(name)),
        choices: Choices::default(),
    };
    let inspect = |name: &str| match call(&files, &Inspect { path: path(name) }).outcome {
        InspectOutcome::Found(i) => i,
        other => panic!("{name}: {other:?}"),
    };

    // 1. read in the inspection cell: what it is, who signed it, what it never gets
    let i = inspect("app.apk");
    assert_eq!(
        (i.source, i.name.as_str()),
        (Source::Apk, label.as_str()),
        "{i:?}"
    );
    assert!(i.installable && !i.verified && i.refusal.is_none(), "{i:?}");
    let (fingerprint, signer) = i.signer.split_once(' ').unwrap_or((&i.signer, ""));
    assert_eq!(fingerprint.len(), 64, "{i:?}");
    assert_eq!(i.publisher, signer);
    println!(
        "FOREIGN-ANDROID: inspected ok ({} {}, signer {} \"{}\" unverified, network suggested: {:?}, never granted: {:?})",
        i.name, i.version, fingerprint, signer, i.suggested.network, i.not_granted
    );

    // 2. refused before anything happens
    for (file, refusal) in [
        ("arm", Refusal::NeedsArm),
        ("tampered", Refusal::BadSignature),
        ("old", Refusal::OldSignature),
        ("unsigned", Refusal::Unsigned),
    ] {
        let r = inspect(&format!("{file}.apk"));
        assert_eq!(r.refusal, Some(refusal), "{file}: {r:?}");
        assert!(!r.installable, "{file}");
        let job = started(call(&files, &install(&format!("{file}.apk"))).outcome);
        assert_eq!(
            end_of(&progress, job).step,
            Step::Failed(Failure::Damaged),
            "{file}"
        );
    }
    assert!(
        asked.lock().unwrap().is_empty(),
        "a refused file asks nobody"
    );
    assert!(!runtime.join(APPS).join(&package).exists());
    println!("FOREIGN-ANDROID: refusals ok (ARM only, changed, JAR signing only, unsigned)");

    // 3. "cancelled" installs nothing
    let job = started(call(&files, &install("app.apk")).outcome);
    assert_eq!(end_of(&progress, job).step, Step::Cancelled);
    assert!(!runtime.join(APPS).join(&package).exists());
    assert_eq!(asked.lock().unwrap()[0].key, "foreign.confirm.install");
    println!("FOREIGN-ANDROID: cancelled ok");

    // 4. confirmed: placed read-only in the area, registered as app.foreign
    authorize.store(true, Ordering::SeqCst);
    let job = started(call(&files, &install("app.apk")).outcome);
    let done = end_of(&progress, job);
    assert_eq!(done.step, Step::Done, "{done:?}");
    assert_eq!(done.apps, vec![id.clone()]);
    let placed = runtime.join(APPS).join(&package).join("base.apk");
    assert_eq!(sha256(&placed), sha256(&downloads.join("app.apk")));
    let meta = std::fs::metadata(&placed).unwrap();
    assert_eq!(meta.permissions().mode() & 0o7777, 0o644);
    assert_eq!(std::os::unix::fs::MetadataExt::uid(&meta), 0);
    let listed = launcher.list();
    let entry = listed.iter().find(|a| a.app == id).unwrap();
    assert_eq!(entry.profile, "app.foreign");
    let uid = entry.uid;
    let apps = call(&settings, &List {}).apps;
    assert_eq!(apps.len(), 1);
    assert_eq!(
        apps[0].choices,
        Choices::default(),
        "nothing granted unasked"
    );
    assert!(Path::new(&apps[0].icon).is_file(), "{:?}", apps[0]);
    println!("FOREIGN-ANDROID: installed ok ({id}, uid {uid})");

    // 5. in its cell, with its window
    launcher.start(&id, Role::Ui, None).unwrap();
    let home = world.path(&format!("appdata/{id}"));
    let probe = wait_file(&home.join("probe.txt"), "end ok");
    assert!(
        line(&probe, "args").starts_with(&format!(
            "-- /opt/ultar-android/bin/ultar-apk-run {package} "
        )),
        "{probe}"
    );
    assert_eq!(line(&probe, "uid"), uid.to_string());
    assert_eq!(line(&probe, "write-area"), "done", "{probe}");
    assert_eq!(line(&probe, "host-secret"), "hidden");
    assert_eq!(
        line(&probe, "net").trim(),
        "lo",
        "no network without the permission"
    );
    assert_eq!(line(&probe, "caps"), "0000000000000000");
    assert_eq!(line(&probe, "nnp"), "1");
    assert_eq!(
        line(&probe, "home-exec"),
        "refused",
        "the app's data is not executable"
    );
    let devices: Vec<&str> = line(&probe, "devices").split_whitespace().collect();
    assert!(
        devices
            .iter()
            .all(|d| ["null", "zero", "full", "random", "urandom", "tty"].contains(d)),
        "{devices:?}"
    );
    assert!(
        !line(&probe, "id-files").contains("=33"),
        "no machine identity in the area: {probe}"
    );
    let deadline = Instant::now() + WINDOW_WAIT;
    let shown = loop {
        let ok = Command::new(shot)
            .arg(&wayland)
            .arg(shot_out)
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            break true;
        }
        if Instant::now() > deadline {
            break false;
        }
        std::thread::sleep(Duration::from_secs(5));
    };
    let log = std::fs::read_to_string(home.join("app.log")).unwrap_or_default();
    if !shown {
        eprintln!("--- app.log\n{log}\n--- launcher\n{:?}", output.of(&id));
    }
    assert!(shown, "no window of the app on the display");
    // native code stays in the area: the data has only a link to it
    let lib = home.join("android/base.apk_/lib");
    assert_eq!(
        std::fs::read_link(&lib).unwrap(),
        Path::new("/").join(APPS).join(&package).join("lib")
    );
    println!("FOREIGN-ANDROID: window ok ({shot_out})");
    let _ = launcher.stop(&id, Role::Ui);

    // 6. an update from another signer is refused before anyone is asked
    let before = asked.lock().unwrap().len();
    let job = started(call(&files, &install("other.apk")).outcome);
    assert_eq!(
        end_of(&progress, job).step,
        Step::Failed(Failure::SignerChanged)
    );
    assert_eq!(asked.lock().unwrap().len(), before);
    assert_eq!(sha256(&placed), sha256(&downloads.join("app.apk")));
    // the same signer again is fine
    let job = started(call(&files, &install("app.apk")).outcome);
    let done = end_of(&progress, job);
    assert_eq!(done.step, Step::Done, "{done:?}");
    assert_eq!(
        done.apps,
        vec![id.clone()],
        "the same app, updated in place"
    );
    println!("FOREIGN-ANDROID: update rules ok");

    // 7. removal leaves nothing behind
    let removed = call(
        &settings,
        &Remove {
            id: id.clone(),
            keep_data: false,
        },
    );
    assert_eq!(removed.outcome, Outcome::Done);
    assert!(common::wait_until_ok(
        || !call(&settings, &RuntimeState {}).busy
    ));
    assert!(common::wait_until_ok(|| !runtime
        .join(APPS)
        .join(&package)
        .exists()));
    assert!(launcher
        .list()
        .iter()
        .all(|a| !a.app.starts_with("foreign.")));
    assert!(call(&settings, &List {}).apps.is_empty());
    assert!(!home.exists());
    for d in ["icons", "inbox"] {
        assert_eq!(
            std::fs::read_dir(foreign_root.join(d)).unwrap().count(),
            0,
            "{d}"
        );
    }
    assert_eq!(
        std::fs::read_dir(runtime.join(APPS)).unwrap().count(),
        0,
        "no app left in the area"
    );
    println!("FOREIGN-ANDROID: removal ok");

    let _ = Command::new("kill")
        .args(["-TERM", &service.id().to_string()])
        .status();
    assert_eq!(service.wait().unwrap().code(), Some(0));
    let _ = compositor.kill();
    let _ = compositor.wait();
    stop.store(true, Ordering::SeqCst);
    life.request_stop(StopCause::Request);
    drop(_broker);
    serving.join().unwrap();
    println!("FOREIGN-ANDROID-E2E: ALLES OK");
}
