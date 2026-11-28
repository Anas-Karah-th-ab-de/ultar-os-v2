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
mod foreign;

use common::{root, World, WAIT};
use foreign::{call, client, dpkg_info, end_of, installed, line, started, text, wait_file, Device};
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use ultar_bus::messages::foreign::{
    Choices, Inspect, InspectOutcome, Install, InstallFrom, InstallOutcome, List, NetworkChoice,
    Outcome, Progress, Remove, RuntimeState, SetChoices, Source, Step,
};
use ultar_bus::messages::launcher::Role;
use ultar_bus::{Client, TextArg};

/// Patience for the package manager.
const APT_WAIT: Duration = Duration::from_secs(600);

/// The identities of this test, one socket each.
const IDENTITIES: [&str; 6] = [
    "system.launcher",
    "system.foreign",
    "system.sysdialog",
    "app.files",
    "app.settings",
    "app.some-store-app",
];

/// The editor's app.
const MOUSEPAD: &str = "foreign.deb-mousepad";

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

/// The installation of the file `path` with the default choices.
fn install(path: &str) -> Install {
    Install {
        from: InstallFrom::File(path.to_owned()),
        choices: Choices::default(),
    }
}

/// What the steps share: the world, the running device, the callers and the places.
struct Scene {
    world: World,
    device: Device,
    files: Client,
    settings: Client,
    stranger: Client,
    progress: ultar_bus::TypedSubscription<Progress>,
    foreign_root: PathBuf,
    runtime: PathBuf,
    /// The person's package `mousepad.deb` in the downloads.
    mousepad: String,
    /// The area's packages before the test.
    before: BTreeSet<String>,
}

impl Scene {
    fn end_of(&self, job: u64) -> Progress {
        end_of(&self.progress, job, APT_WAIT)
    }
}

/// The area (a copy), the bridge, the person's downloads, a secret of the host, the fake
/// group tree; the launcher's configuration.
fn prepare(world: &World, area: &str, deb: &str) -> String {
    let runtime = world.path("foreign/linux");
    foreign::copy_area(area, &runtime);
    let suid = runtime.join("usr/local/bin/ultar-suid-id");
    std::fs::create_dir_all(suid.parent().unwrap()).unwrap();
    std::fs::copy(runtime.join("usr/bin/id"), &suid).unwrap();
    std::fs::set_permissions(&suid, std::fs::Permissions::from_mode(0o4755)).unwrap();
    let bridge = foreign::fake_bridge(world, &probe_script(world));
    let downloads = world.path("areas/downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::copy(deb, downloads.join("mousepad.deb")).unwrap();
    std::fs::write(world.path("secret"), b"host secret").unwrap();
    std::fs::write("/tmp/ultar-host-marker", b"host").unwrap();
    std::os::unix::fs::symlink("/etc/shadow", downloads.join("shadow.deb")).unwrap();
    let tree = world.fake_group_tree();
    for app in [MOUSEPAD, "foreign.deb-ultar-evil"] {
        foreign::group_files(&tree, app);
    }
    format!(
        "{}foreign_runtime = \"{}\"\nforeign_bridge = \"{}\"\n",
        world.config_text(&[], Some(&tree)),
        runtime.display(),
        bridge.display()
    )
}

/// The device with the service, once it sees the area.
fn start(mut world: World, area: &str, deb: &str, bin: &str) -> Scene {
    let config = prepare(&world, area, deb);
    let foreign_root = world.path("foreign");
    let before = dpkg_info(&foreign_root.join("linux"));
    let device = Device::start(&mut world, &IDENTITIES, &config, bin, &foreign_root);
    let files = client(&world, "app.files");
    let settings = client(&world, "app.settings");
    let stranger = client(&world, "app.some-store-app");
    let progress = files.subscribe::<Progress>().unwrap();
    assert!(common::wait_until_ok(|| settings
        .call(&RuntimeState {}, Duration::from_secs(1))
        .is_ok_and(|r| r.installed && r.version.starts_with("2"))));
    let mousepad = text(&world.path("areas/downloads/mousepad.deb"));
    Scene {
        runtime: foreign_root.join("linux"),
        foreign_root,
        world,
        device,
        files,
        settings,
        stranger,
        progress,
        mousepad,
        before,
    }
}

/// 1. refused before anything happens.
fn refusals(s: &Scene) {
    assert!(
        s.stranger.call(&install(&s.mousepad), WAIT).is_err(),
        "no rule, no call"
    );
    assert!(s.stranger.call(&List {}, WAIT).is_err());
    assert!(
        s.files.call(&List {}, WAIT).is_err(),
        "Files only inspects and installs"
    );
    let inspect = |p: &str| call(&s.files, &Inspect { path: p.to_owned() }).outcome;
    assert_eq!(inspect("/etc/shadow"), InspectOutcome::Invalid);
    assert_eq!(
        inspect(&text(&s.foreign_root.join("state.toml"))),
        InspectOutcome::Invalid
    );
    let link = text(&s.world.path("areas/downloads/shadow.deb"));
    assert_ne!(
        std::mem::discriminant(&inspect(&link)),
        std::mem::discriminant(&InspectOutcome::Found(dummy()))
    );
    assert_eq!(
        call(&s.files, &install("/etc/passwd")).outcome,
        InstallOutcome::Invalid
    );
    match inspect(&s.mousepad) {
        InspectOutcome::Found(i) => {
            assert_eq!((i.source, i.name.as_str()), (Source::Deb, "mousepad"));
            assert!(i.installable && !i.verified, "{i:?}");
        }
        other => panic!("{other:?}"),
    }
    println!("FOREIGN: refusals ok");
}

/// 2. "cancelled" installs nothing.
fn cancelled(s: &Scene) {
    s.device.authorize(false);
    let job = started(call(&s.files, &install(&s.mousepad)).outcome);
    assert_eq!(s.end_of(job).step, Step::Cancelled);
    assert!(!installed(&s.runtime, "mousepad"));
    assert!(s
        .device
        .launcher
        .list()
        .iter()
        .all(|a| !a.app.starts_with("foreign.")));
    let asked = s.device.dialog.asked.lock().unwrap();
    assert_eq!(asked[0].key, "foreign.confirm.install");
    assert_eq!(asked[0].args[0], TextArg::Verbatim("mousepad".into()));
    assert_eq!(asked[0].args[1], TextArg::Key("foreign.source.deb".into()));
    drop(asked);
    println!("FOREIGN: cancelled ok");
}

/// 3. confirmed: installed in the cell, registered as app.foreign; the app's user id.
fn confirmed(s: &Scene) -> u32 {
    s.device.authorize(true);
    let job = started(call(&s.files, &install(&s.mousepad)).outcome);
    let done = s.end_of(job);
    assert_eq!(done.step, Step::Done, "{done:?}");
    assert_eq!(done.apps, vec![MOUSEPAD.to_owned()]);
    assert!(installed(&s.runtime, "mousepad"));
    let listed = s.device.launcher.list();
    let app = listed.iter().find(|a| a.app == MOUSEPAD).unwrap();
    assert_eq!(app.profile, "app.foreign");
    let uid = app.uid;
    assert!((20000..=20009).contains(&uid));
    let apps = call(&s.settings, &List {}).apps;
    assert_eq!(apps.len(), 1);
    assert!(
        apps[0]
            .icon
            .starts_with(&text(&s.foreign_root.join("icons"))),
        "{:?}",
        apps[0]
    );
    assert!(Path::new(&apps[0].icon).is_file());
    // its own title bar (GTK): the compositor's right for exactly this app
    let rights = std::fs::read_to_string(s.world.path("share/compositor-apps.toml")).unwrap();
    assert!(
        rights
            .lines()
            .any(|l| l.starts_with(&format!("{uid} ")) && l.contains("ui.client-header")),
        "{rights}"
    );
    println!("FOREIGN: installed ok");
    uid
}

/// 4. the app in its cell: what it sees of the host.
fn cell_isolation(probe: &str, uid: u32) {
    assert!(
        line(probe, "args").starts_with("-- /usr/bin/mousepad"),
        "{probe}"
    );
    assert_eq!(line(probe, "uid"), uid.to_string());
    assert_eq!(line(probe, "write-area"), "done", "{probe}");
    assert_eq!(line(probe, "host-secret"), "hidden");
    // only the mount point of the shared compositor directory, nothing of the host's scratch
    assert!(
        ["", "share"].contains(&line(probe, "host-dir").trim()),
        "{probe}"
    );
    assert_eq!(line(probe, "bus"), "none");
    assert_eq!(line(probe, "bus-sockets"), "");
    let devices: BTreeSet<&str> = line(probe, "devices").split_whitespace().collect();
    // ptmx: the cell's own terminals (a devpts instance of its own, or on the Ultar kernel slaves that
    // open for their owner only), never the host's
    let allowed: BTreeSet<&str> =
        ["null", "zero", "full", "random", "urandom", "tty", "ptmx"].into();
    assert!(devices.is_subset(&allowed), "{devices:?}");
    assert_eq!(
        line(probe, "net").trim(),
        "lo",
        "no network without the permission"
    );
    assert_eq!(line(probe, "caps"), "0000000000000000");
    assert_eq!(line(probe, "nnp"), "1");
    assert!(line(probe, "pids").parse::<u32>().unwrap() < 10, "{probe}");
    assert_eq!(
        line(probe, "suid"),
        uid.to_string(),
        "set-id is ignored (nosuid)"
    );
}

/// 4. the app in its cell: its own home, `/tmp` and environment, no way to more rights.
fn cell_environment(probe: &str) {
    assert_eq!(line(probe, "home"), "writable");
    assert_eq!(line(probe, "tmp"), "private");
    assert_eq!(line(probe, "tmp-write"), "ok");
    assert_eq!(line(probe, "hostname"), "ultar-linux");
    assert_eq!(line(probe, "xdg"), "/tmp/.run wayland-0 wayland wayland");
    assert!(line(probe, "lang").ends_with(".UTF-8"));
    // Linux hosts only: without a seccomp filter a cell may make a user namespace (every
    // app cell, not only these; the Ultar kernel has none). Reported, see the docs.
    if line(probe, "userns") != "refused" {
        println!("FOREIGN: note: this Linux host lets a cell create a user namespace (open point)");
    }
    assert_eq!(line(probe, "mount"), "refused");
}

/// 4. the app in its cell.
fn in_its_cell(s: &Scene, uid: u32) {
    s.device.launcher.start(MOUSEPAD, Role::Ui, None).unwrap();
    let probe = wait_file(
        &s.world.path("appdata/foreign.deb-mousepad/probe.txt"),
        "end ok",
        WAIT,
    );
    cell_isolation(&probe, uid);
    cell_environment(&probe);
    println!("FOREIGN: cell ok (uid {uid})");
}

/// 5. more for the app is asked again; the network without the barrier is refused.
fn permissions(s: &Scene) {
    let launcher = &s.device.launcher;
    let _ = launcher.stop(MOUSEPAD, Role::Ui);
    let net = Choices {
        network: NetworkChoice::Internet,
        ..Choices::default()
    };
    let set = |c: &Choices| SetChoices {
        id: MOUSEPAD.into(),
        choices: c.clone(),
    };
    s.device.authorize(false);
    let Outcome::Asking(job) = call(&s.settings, &set(&net)).outcome else {
        panic!("granting more must ask");
    };
    assert_eq!(s.end_of(job).step, Step::Cancelled);
    assert_eq!(
        call(&s.settings, &List {}).apps[0].choices,
        Choices::default()
    );
    s.device.authorize(true);
    let Outcome::Asking(job) = call(&s.settings, &set(&net)).outcome else {
        panic!("granting more must ask");
    };
    assert_eq!(s.end_of(job).step, Step::Done);
    assert_eq!(
        s.device.dialog.asked.lock().unwrap().last().unwrap().key,
        "foreign.confirm.change"
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        launcher.start(MOUSEPAD, Role::Ui, None).is_err(),
        "network without the kernel's barrier: no start, no unsafe mode"
    );
    assert_eq!(
        call(&s.settings, &set(&Choices::default())).outcome,
        Outcome::Done
    );
    launcher.start(MOUSEPAD, Role::Ui, None).unwrap();
    let _ = launcher.stop(MOUSEPAD, Role::Ui);
    println!("FOREIGN: permissions ok");
}

/// 6. what the hostile package's script did stays in the installation cell.
fn hostile_scripts(s: &Scene, domainname: &str) {
    assert!(
        !s.world.path("escaped").exists(),
        "a script wrote outside the area"
    );
    assert_eq!(
        std::fs::read_to_string("/proc/sys/kernel/domainname").unwrap(),
        domainname,
        "a script changed the host"
    );
    let evil_net = std::fs::read_to_string(s.runtime.join("var/tmp/evil-net")).unwrap();
    let interfaces: Vec<&str> = evil_net
        .lines()
        .skip(2)
        .filter_map(|l| l.split(':').next())
        .map(str::trim)
        .collect();
    assert_eq!(interfaces, ["lo"], "the installation cell has no network");
    assert!(
        !s.runtime.join("var/tmp/evil-mount").exists(),
        "a script mounted"
    );
    let bounded = std::fs::read_to_string(s.runtime.join("var/tmp/evil-caps")).unwrap();
    assert!(bounded.trim().ends_with("000000000000001b"), "{bounded}");
}

/// 6. a hostile package stays in the installation cell.
fn hostile(s: &Scene, domainname: &str) {
    let evil = evil_deb(&s.world);
    let job = started(call(&s.files, &install(&text(&evil))).outcome);
    let done = s.end_of(job);
    assert_eq!(done.step, Step::Done, "{done:?}");
    assert_eq!(
        done.apps,
        vec!["foreign.deb-ultar-evil".to_owned()],
        "the entry pointing out is dropped"
    );
    hostile_scripts(s, domainname);
    let mode = std::fs::metadata(s.runtime.join("usr/bin/ultar-evil"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o6000, 0, "set-id bits stay off in the area");
    let evil_app = call(&s.settings, &List {})
        .apps
        .into_iter()
        .find(|a| a.id == "foreign.deb-ultar-evil")
        .unwrap();
    assert!(
        evil_app.icon.is_empty(),
        "an icon outside the area is not taken"
    );
    println!("FOREIGN: hostile package ok");
}

/// Entries of a directory of the service.
fn entries(dir: &Path) -> usize {
    std::fs::read_dir(dir).unwrap().count()
}

/// 7. removal leaves nothing behind.
fn removal(s: &Scene) {
    for id in ["foreign.deb-ultar-evil", MOUSEPAD] {
        let removed = call(
            &s.settings,
            &Remove {
                id: id.into(),
                keep_data: false,
            },
        );
        assert_eq!(removed.outcome, Outcome::Done, "{id}");
        assert!(common::wait_until_ok(|| !call(
            &s.settings,
            &RuntimeState {}
        )
        .busy));
    }
    assert!(common::wait_until_ok(
        || !installed(&s.runtime, "mousepad") && !installed(&s.runtime, "ultar-evil")
    ));
    assert!(s
        .device
        .launcher
        .list()
        .iter()
        .all(|a| !a.app.starts_with("foreign.")));
    assert!(call(&s.settings, &List {}).apps.is_empty());
    assert!(!s.world.path("appdata/foreign.deb-mousepad").exists());
    assert_eq!(entries(&s.foreign_root.join("icons")), 0);
    assert_eq!(entries(&s.foreign_root.join("inbox")), 0);
    assert!(!s.runtime.join("usr/bin/mousepad").exists());
    assert_eq!(
        dpkg_info(&s.runtime),
        s.before,
        "the area's packages are as before"
    );
    println!("FOREIGN: removal ok");
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
    let domainname = std::fs::read_to_string("/proc/sys/kernel/domainname").unwrap();
    let scene = start(World::new("foreign"), &area, &deb, &bin);
    refusals(&scene);
    cancelled(&scene);
    let uid = confirmed(&scene);
    in_its_cell(&scene, uid);
    permissions(&scene);
    hostile(&scene, &domainname);
    removal(&scene);
    let Scene { device, .. } = scene;
    device.stop();
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
        signer: String::new(),
        refusal: None,
        not_granted: Vec::new(),
    }
}
