//! End to end on Linux (root, real cells): a Linux program from a `.deb` of the person
//! through `ultar-foreign` and the launcher, with the shipped bus policy.
//!
//! Needs the Linux area built by `tools/foreign/make-linux-runtime.sh` (with
//! `TEST_PACKAGE=mousepad`) and the service binary; `tools/foreign/linux-e2e.sh` sets:
//!
//! * `ULTAR_FOREIGN_RUNTIME` – the area's tree (copied here, the original stays as it is)
//! * `ULTAR_FOREIGN_DEB` – the saved `mousepad.deb`
//! * `ULTAR_FOREIGN_BIN` – `target/…/ultar-foreign`
//!
//! Without them (or without root) the test says so and passes.
//!
//! What it checks: refusals before anything happens (callers without a rule, files
//! outside the person's areas, links), "cancelled" in the dialog installs nothing, the
//! confirmed installation registers an `app.foreign` app, the app runs as its own user in
//! its cell (area read-only, own home, private `/tmp`, no bus, only harmless devices, no
//! network, no capabilities, set-id ignored), granting it more asks again and the
//! network without the kernel's barrier is refused (no unsafe mode), a hostile package's
//! scripts stay in the installation cell, and removal leaves nothing behind.

mod common;

use common::{root, World, WAIT};
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_bus::messages::foreign::{
    Choices, InspectOutcome, InstallFrom, InstallOutcome, NetworkChoice, Outcome, Progress, Remove,
    RuntimeState, SetChoices, Source, Step,
};
use ultar_bus::messages::foreign::{Inspect, Install, List};
use ultar_bus::messages::launcher::Role;
use ultar_bus::messages::sysdialog::{AuthResult, Authenticate, AuthenticateReply};
use ultar_bus::{
    Broker, Call, Client, ClientOptions, FixedIdentity, Identity, Policy, Text, TextArg,
};
use ultar_launcher::service::{self, Admins};
use ultar_service::{Lifecycle, StopCause};

/// Patience for the package manager.
const APT_WAIT: Duration = Duration::from_secs(600);

fn shipped_policy() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../rootfs/etc/ultar/bus-policy.toml")
}

/// The identities of this test, one socket each.
const IDENTITIES: [&str; 6] = [
    "system.launcher",
    "system.foreign",
    "system.sysdialog",
    "app.files",
    "app.settings",
    "app.some-store-app",
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

/// The system dialog: answers every question with `authorize` and keeps the questions.
struct Dialog {
    authorize: Arc<AtomicBool>,
    asked: Arc<Mutex<Vec<Text>>>,
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

/// Waits for the end of `job` on `foreign.progress`.
fn end_of(progress: &ultar_bus::TypedSubscription<Progress>, job: u64) -> Progress {
    let deadline = Instant::now() + APT_WAIT;
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

fn dpkg_info(runtime: &Path) -> BTreeSet<String> {
    std::fs::read_dir(runtime.join("var/lib/dpkg/info"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

fn installed(runtime: &Path, package: &str) -> bool {
    ["", ":amd64", ":arm64", ":all"].iter().any(|arch| {
        runtime
            .join(format!("var/lib/dpkg/info/{package}{arch}.list"))
            .exists()
    })
}

/// The fake Wayland bridge: what the app's cell looks like from inside, written to its
/// home before the program runs (the real bridge is `ultar-wayland`, stage P1).
fn probe_script(world: &World) -> String {
    let secret = world.path("secret");
    let scratch = world.dir.display();
    format!(
        r#"#!/bin/sh
out="$HOME/probe.txt"
: >"$out"
r() {{ echo "$1 $2" >>"$out"; }}
r args "$*"
r uid "$(id -u)"
r gid "$(id -g)"
for p in /usr/bin/ultar-probe /etc/ultar-probe /var/lib/dpkg/ultar-probe /ultar-probe; do
  if touch "$p" 2>/dev/null; then r write-area "FAIL:$p"; fi
done
r write-area done
if cat "{secret}" >/dev/null 2>&1; then r host-secret FAIL; else r host-secret hidden; fi
r host-dir "$(ls -A "{scratch}" 2>/dev/null | tr '\n' ' ')"
if [ -e /run/ultar/appbus ] || [ -e /run/ultar/bus ]; then r bus FAIL; else r bus none; fi
r bus-sockets "$(find / -xdev -name bus.sock 2>/dev/null; find /run /tmp -type s -name 'bus*' 2>/dev/null)"
r devices "$(find /dev \( -type c -o -type b \) -printf '%f ' 2>/dev/null)"
r net "$(tail -n +3 /proc/net/dev | cut -d: -f1 | tr -d ' ' | tr '\n' ' ')"
r caps "$(grep CapEff /proc/self/status | tr -s '\t ' ' ' | cut -d' ' -f2)"
r capbnd "$(grep CapBnd /proc/self/status | tr -s '\t ' ' ' | cut -d' ' -f2)"
r nnp "$(grep NoNewPrivs /proc/self/status | tr -s '\t ' ' ' | cut -d' ' -f2)"
r pids "$(ls -d /proc/[0-9]* | wc -l)"
r suid "$(/usr/local/bin/ultar-suid-id -u)"
if echo ok >"$HOME/written" 2>/dev/null; then r home writable; else r home FAIL; fi
if [ -e /tmp/ultar-host-marker ]; then r tmp FAIL; else r tmp private; fi
if echo x >/tmp/ultar-probe 2>/dev/null; then r tmp-write ok; else r tmp-write FAIL; fi
r hostname "$(cat /etc/hostname)"
r xdg "$XDG_RUNTIME_DIR $WAYLAND_DISPLAY $GDK_BACKEND $QT_QPA_PLATFORM"
r lang "$LANG"
if unshare -U -r true 2>/dev/null; then r userns FAIL; else r userns refused; fi
if mount -t tmpfs none /tmp 2>/dev/null; then r mount FAIL; else r mount refused; fi
r end ok
shift
exec "$@"
"#,
        secret = secret.display()
    )
}

/// A hostile package: a set-id program, a link out of the area for its icon, an entry
/// that points out of the area, and a script that tries to get out of the installation cell.
fn evil_deb(world: &World) -> PathBuf {
    let root = world.path("evil-src");
    std::fs::create_dir_all(root.join("DEBIAN")).unwrap();
    std::fs::create_dir_all(root.join("usr/bin")).unwrap();
    std::fs::create_dir_all(root.join("usr/share/applications")).unwrap();
    std::fs::write(
        root.join("DEBIAN/control"),
        "Package: ultar-evil\nVersion: 1.0\nArchitecture: all\nMaintainer: Mallory <m@example.org>\n\
         Description: tries everything\n",
    )
    .unwrap();
    let escaped = world.path("escaped");
    std::fs::write(
        root.join("DEBIAN/postinst"),
        format!(
            "#!/bin/sh\necho pwned > \"{}\" 2>/dev/null\n\
             echo ultar-pwned > /proc/sys/kernel/domainname 2>/dev/null\n\
             cat /proc/net/dev > /var/tmp/evil-net 2>/dev/null\n\
             id -u > /var/tmp/evil-uid\n\
             grep CapBnd /proc/self/status > /var/tmp/evil-caps\n\
             mount -t tmpfs none /mnt 2>/dev/null && echo mounted > /var/tmp/evil-mount\n\
             chmod u+s /usr/bin/ultar-evil 2>/dev/null\nexit 0\n",
            escaped.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(
        root.join("DEBIAN/postinst"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::write(root.join("usr/bin/ultar-evil"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(
        root.join("usr/bin/ultar-evil"),
        std::fs::Permissions::from_mode(0o4755),
    )
    .unwrap();
    std::fs::write(
        root.join("usr/share/applications/ultar-evil.desktop"),
        "[Desktop Entry]\nType=Application\nName=Evil\nExec=ultar-evil %U\nIcon=../../../../../etc/shadow\n",
    )
    .unwrap();
    std::fs::write(
        root.join("usr/share/applications/ultar-evil-out.desktop"),
        "[Desktop Entry]\nType=Application\nName=Out\nExec=/../../../../bin/sh -c id\n",
    )
    .unwrap();
    let out = world.path("areas/downloads/ultar-evil_1.0_all.deb");
    let built = Command::new("dpkg-deb")
        .args(["--root-owner-group", "--build"])
        .arg(&root)
        .arg(&out)
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(built.success());
    out
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

#[test]
fn a_linux_program_goes_in_runs_in_its_cell_and_leaves_without_trace() {
    let (Ok(area), Ok(deb), Ok(bin)) = (
        std::env::var("ULTAR_FOREIGN_RUNTIME"),
        std::env::var("ULTAR_FOREIGN_DEB"),
        std::env::var("ULTAR_FOREIGN_BIN"),
    ) else {
        eprintln!("skipped: ULTAR_FOREIGN_RUNTIME, ULTAR_FOREIGN_DEB, ULTAR_FOREIGN_BIN not set");
        return;
    };
    if !root() {
        eprintln!("skipped: real cells need root");
        return;
    }
    let mut world = World::new("foreign");
    // the area (a copy), the bridge, the person's downloads, a secret of the host
    let foreign_root = world.path("foreign");
    let runtime = foreign_root.join("linux");
    std::fs::create_dir_all(&foreign_root).unwrap();
    let copied = Command::new("cp")
        .args(["-a", &area])
        .arg(&runtime)
        .status()
        .unwrap();
    assert!(copied.success());
    let suid = runtime.join("usr/local/bin/ultar-suid-id");
    std::fs::create_dir_all(suid.parent().unwrap()).unwrap();
    std::fs::copy(runtime.join("usr/bin/id"), &suid).unwrap();
    std::fs::set_permissions(&suid, std::fs::Permissions::from_mode(0o4755)).unwrap();
    let bridge = world.path("bridge");
    std::fs::create_dir_all(&bridge).unwrap();
    std::fs::write(bridge.join("ultar-wayland"), probe_script(&world)).unwrap();
    std::fs::set_permissions(
        bridge.join("ultar-wayland"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let downloads = world.path("areas/downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::copy(&deb, downloads.join("mousepad.deb")).unwrap();
    std::fs::write(world.path("secret"), b"host secret").unwrap();
    std::fs::write("/tmp/ultar-host-marker", b"host").unwrap();
    std::os::unix::fs::symlink("/etc/shadow", downloads.join("shadow.deb")).unwrap();
    let tree = world.fake_group_tree();
    for app in ["foreign.deb-mousepad", "foreign.deb-ultar-evil"] {
        let dir = tree.join("apps").join(app);
        std::fs::create_dir_all(&dir).unwrap();
        for file in ["cgroup.procs", "memory.max", "pids.max"] {
            std::fs::write(dir.join(file), b"").unwrap();
        }
    }
    let before = dpkg_info(&runtime);
    let domainname = std::fs::read_to_string("/proc/sys/kernel/domainname").unwrap();

    // the bus, the launcher on it, the system dialog, the service
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
    let dialog = dialog(&world, Arc::clone(&stop));
    let mut service = Command::new(&bin)
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
    let stranger = client(&world, "app.some-store-app");
    let progress = files.subscribe::<Progress>().unwrap();
    assert!(common::wait_until_ok(|| settings
        .call(&RuntimeState {}, Duration::from_secs(1))
        .is_ok_and(|r| r.installed && r.version.starts_with("2"))));

    // 1. refused before anything happens
    let path = |p: &Path| p.display().to_string();
    let mousepad = path(&downloads.join("mousepad.deb"));
    let install = |p: &str| Install {
        from: InstallFrom::File(p.to_owned()),
        choices: Choices::default(),
    };
    assert!(
        stranger.call(&install(&mousepad), WAIT).is_err(),
        "no rule, no call"
    );
    assert!(stranger.call(&List {}, WAIT).is_err());
    assert!(
        files.call(&List {}, WAIT).is_err(),
        "Files only inspects and installs"
    );
    let inspect = |p: &str| call(&files, &Inspect { path: p.to_owned() }).outcome;
    assert_eq!(inspect("/etc/shadow"), InspectOutcome::Invalid);
    assert_eq!(
        inspect(&path(&foreign_root.join("state.toml"))),
        InspectOutcome::Invalid
    );
    assert_ne!(
        std::mem::discriminant(&inspect(&path(&downloads.join("shadow.deb")))),
        std::mem::discriminant(&InspectOutcome::Found(dummy()))
    );
    assert_eq!(
        call(&files, &install("/etc/passwd")).outcome,
        InstallOutcome::Invalid
    );
    match inspect(&mousepad) {
        InspectOutcome::Found(i) => {
            assert_eq!((i.source, i.name.as_str()), (Source::Deb, "mousepad"));
            assert!(i.installable && !i.verified, "{i:?}");
        }
        other => panic!("{other:?}"),
    }
    println!("FOREIGN: refusals ok");

    // 2. "cancelled" installs nothing
    dialog.authorize.store(false, Ordering::SeqCst);
    let job = started(call(&files, &install(&mousepad)).outcome);
    assert_eq!(end_of(&progress, job).step, Step::Cancelled);
    assert!(!installed(&runtime, "mousepad"));
    assert!(launcher
        .list()
        .iter()
        .all(|a| !a.app.starts_with("foreign.")));
    {
        let asked = dialog.asked.lock().unwrap();
        assert_eq!(asked[0].key, "foreign.confirm.install");
        assert_eq!(asked[0].args[0], TextArg::Verbatim("mousepad".into()));
        assert_eq!(asked[0].args[1], TextArg::Key("foreign.source.deb".into()));
    }
    println!("FOREIGN: cancelled ok");

    // 3. confirmed: installed in the cell, registered as app.foreign
    dialog.authorize.store(true, Ordering::SeqCst);
    let job = started(call(&files, &install(&mousepad)).outcome);
    let done = end_of(&progress, job);
    assert_eq!(done.step, Step::Done, "{done:?}");
    assert_eq!(done.apps, vec!["foreign.deb-mousepad".to_owned()]);
    assert!(installed(&runtime, "mousepad"));
    let listed = launcher.list();
    let app = listed
        .iter()
        .find(|a| a.app == "foreign.deb-mousepad")
        .unwrap();
    assert_eq!(app.profile, "app.foreign");
    let uid = app.uid;
    assert!((20000..=20009).contains(&uid));
    let apps = call(&settings, &List {}).apps;
    assert_eq!(apps.len(), 1);
    assert!(
        apps[0].icon.starts_with(&path(&foreign_root.join("icons"))),
        "{:?}",
        apps[0]
    );
    assert!(Path::new(&apps[0].icon).is_file());
    println!("FOREIGN: installed ok");

    // 4. the app in its cell
    launcher
        .start("foreign.deb-mousepad", Role::Ui, None)
        .unwrap();
    let probe = wait_file(
        &world.path("appdata/foreign.deb-mousepad/probe.txt"),
        "end ok",
    );
    assert!(
        line(&probe, "args").starts_with("-- /usr/bin/mousepad"),
        "{probe}"
    );
    assert_eq!(line(&probe, "uid"), uid.to_string());
    assert_eq!(line(&probe, "write-area"), "done", "{probe}");
    assert_eq!(line(&probe, "host-secret"), "hidden");
    // only the mount point of the shared compositor directory, nothing of the host's scratch
    assert!(
        ["", "share"].contains(&line(&probe, "host-dir").trim()),
        "{probe}"
    );
    assert_eq!(line(&probe, "bus"), "none");
    assert_eq!(line(&probe, "bus-sockets"), "");
    let devices: BTreeSet<&str> = line(&probe, "devices").split_whitespace().collect();
    let allowed: BTreeSet<&str> = ["null", "zero", "full", "random", "urandom", "tty"].into();
    assert!(devices.is_subset(&allowed), "{devices:?}");
    assert_eq!(
        line(&probe, "net").trim(),
        "lo",
        "no network without the permission"
    );
    assert_eq!(line(&probe, "caps"), "0000000000000000");
    assert_eq!(line(&probe, "nnp"), "1");
    assert!(line(&probe, "pids").parse::<u32>().unwrap() < 10, "{probe}");
    assert_eq!(
        line(&probe, "suid"),
        uid.to_string(),
        "set-id is ignored (nosuid)"
    );
    assert_eq!(line(&probe, "home"), "writable");
    assert_eq!(line(&probe, "tmp"), "private");
    assert_eq!(line(&probe, "tmp-write"), "ok");
    assert_eq!(line(&probe, "hostname"), "ultar-linux");
    assert_eq!(line(&probe, "xdg"), "/tmp/.run wayland-0 wayland wayland");
    assert!(line(&probe, "lang").ends_with(".UTF-8"));
    // Linux hosts only: without a seccomp filter a cell may make a user namespace (every
    // app cell, not only these; the Ultar kernel has none). Reported, see the docs.
    if line(&probe, "userns") != "refused" {
        println!("FOREIGN: note: this Linux host lets a cell create a user namespace (open point)");
    }
    assert_eq!(line(&probe, "mount"), "refused");
    println!("FOREIGN: cell ok (uid {uid})");

    // 5. more for the app is asked again; the network without the barrier is refused
    let _ = launcher.stop("foreign.deb-mousepad", Role::Ui);
    let net = Choices {
        network: NetworkChoice::Internet,
        ..Choices::default()
    };
    let set = |c: &Choices| SetChoices {
        id: "foreign.deb-mousepad".into(),
        choices: c.clone(),
    };
    dialog.authorize.store(false, Ordering::SeqCst);
    let Outcome::Asking(job) = call(&settings, &set(&net)).outcome else {
        panic!("granting more must ask");
    };
    assert_eq!(end_of(&progress, job).step, Step::Cancelled);
    assert_eq!(
        call(&settings, &List {}).apps[0].choices,
        Choices::default()
    );
    dialog.authorize.store(true, Ordering::SeqCst);
    let Outcome::Asking(job) = call(&settings, &set(&net)).outcome else {
        panic!("granting more must ask");
    };
    assert_eq!(end_of(&progress, job).step, Step::Done);
    assert_eq!(
        dialog.asked.lock().unwrap().last().unwrap().key,
        "foreign.confirm.change"
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        launcher
            .start("foreign.deb-mousepad", Role::Ui, None)
            .is_err(),
        "network without the kernel's barrier: no start, no unsafe mode"
    );
    assert_eq!(
        call(&settings, &set(&Choices::default())).outcome,
        Outcome::Done
    );
    launcher
        .start("foreign.deb-mousepad", Role::Ui, None)
        .unwrap();
    let _ = launcher.stop("foreign.deb-mousepad", Role::Ui);
    println!("FOREIGN: permissions ok");

    // 6. a hostile package stays in the installation cell
    let evil = evil_deb(&world);
    let job = started(call(&files, &install(&path(&evil))).outcome);
    let done = end_of(&progress, job);
    assert_eq!(done.step, Step::Done, "{done:?}");
    assert_eq!(
        done.apps,
        vec!["foreign.deb-ultar-evil".to_owned()],
        "the entry pointing out is dropped"
    );
    assert!(
        !world.path("escaped").exists(),
        "a script wrote outside the area"
    );
    assert_eq!(
        std::fs::read_to_string("/proc/sys/kernel/domainname").unwrap(),
        domainname,
        "a script changed the host"
    );
    let evil_net = std::fs::read_to_string(runtime.join("var/tmp/evil-net")).unwrap();
    let interfaces: Vec<&str> = evil_net
        .lines()
        .skip(2)
        .filter_map(|l| l.split(':').next())
        .map(str::trim)
        .collect();
    assert_eq!(interfaces, ["lo"], "the installation cell has no network");
    assert!(
        !runtime.join("var/tmp/evil-mount").exists(),
        "a script mounted"
    );
    let bounded = std::fs::read_to_string(runtime.join("var/tmp/evil-caps")).unwrap();
    assert!(bounded.trim().ends_with("00000000000000db"), "{bounded}");
    let mode = std::fs::metadata(runtime.join("usr/bin/ultar-evil"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o6000, 0, "set-id bits stay off in the area");
    let evil_app = call(&settings, &List {})
        .apps
        .into_iter()
        .find(|a| a.id == "foreign.deb-ultar-evil")
        .unwrap();
    assert!(
        evil_app.icon.is_empty(),
        "an icon outside the area is not taken"
    );
    println!("FOREIGN: hostile package ok");

    // 7. removal leaves nothing behind
    for id in ["foreign.deb-ultar-evil", "foreign.deb-mousepad"] {
        let removed = call(
            &settings,
            &Remove {
                id: id.into(),
                keep_data: false,
            },
        );
        assert_eq!(removed.outcome, Outcome::Done, "{id}");
        assert!(common::wait_until_ok(
            || !call(&settings, &RuntimeState {}).busy
        ));
    }
    assert!(common::wait_until_ok(
        || !installed(&runtime, "mousepad") && !installed(&runtime, "ultar-evil")
    ));
    assert!(launcher
        .list()
        .iter()
        .all(|a| !a.app.starts_with("foreign.")));
    assert!(call(&settings, &List {}).apps.is_empty());
    assert!(!world.path("appdata/foreign.deb-mousepad").exists());
    assert_eq!(
        std::fs::read_dir(foreign_root.join("icons"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        std::fs::read_dir(foreign_root.join("inbox"))
            .unwrap()
            .count(),
        0
    );
    assert!(!runtime.join("usr/bin/mousepad").exists());
    assert_eq!(
        dpkg_info(&runtime),
        before,
        "the area's packages are as before"
    );
    println!("FOREIGN: removal ok");

    let _ = Command::new("kill")
        .args(["-TERM", &service.id().to_string()])
        .status();
    let code = service.wait().unwrap().code();
    assert_eq!(code, Some(0));
    stop.store(true, Ordering::SeqCst);
    life.request_stop(StopCause::Request);
    drop(_broker);
    serving.join().unwrap();
    let _ = std::fs::remove_file("/tmp/ultar-host-marker");
    println!("FOREIGN-LINUX-E2E: ALLES OK");
}

fn dummy() -> ultar_bus::messages::foreign::Inspected {
    ultar_bus::messages::foreign::Inspected {
        source: Source::Unknown,
        name: String::new(),
        version: String::new(),
        publisher: String::new(),
        size_bytes: 0,
        verified: false,
        installable: false,
        suggested: Choices::default(),
    }
}
