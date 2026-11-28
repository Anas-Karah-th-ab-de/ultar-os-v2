//! End to end on Linux (root, real cells): Windows programs (`.exe`, `.msi`) of the person
//! through `ultar-foreign` and the launcher – stage P4 of `docs/FREMD-APPS.md`.
//!
//! `tools/foreign/windows-e2e.sh` builds the Linux area with the component „Windows
//! programs“ (and, for this test only, Xvfb/xwd for a screenshot) and sets:
//!
//! * `ULTAR_FOREIGN_RUNTIME` – the area's tree (copied here)
//! * `ULTAR_FOREIGN_BIN` – `target/…/ultar-foreign`
//! * `ULTAR_WINDOWS_EXE` – an NSIS installer (Notepad++), `ULTAR_WINDOWS_MSI` – an MSI (7-Zip)
//! * `ULTAR_WINDOWS_SHOTS` – where the screenshots (`*.xwd`) go
//!
//! Without them (or without root) the test says so and passes.
//!
//! What it checks: the files are described from their own data (unverified), "cancelled"
//! installs nothing, the confirmed installation keeps the installer as the app's own and
//! registers an `app.foreign` app of kind `compat-exe`; on its first start the program
//! installs itself **in its own cell** (its own user, no network, the installer read-only,
//! only drive `C:`, the host invisible also through `\\?\unix\`), its window shows (screenshot),
//! and removal leaves nothing behind (data, installer, icon; the Linux area unchanged).

mod common;

use common::{root, World, WAIT};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_bus::messages::foreign::{
    Choices, Inspect, InspectOutcome, Install, InstallFrom, InstallOutcome, List, Outcome,
    Progress, Remove, RuntimeState, Source, Step,
};
use ultar_bus::messages::launcher::Role;
use ultar_bus::messages::sysdialog::{AuthResult, Authenticate, AuthenticateReply};
use ultar_bus::{Broker, Call, Client, ClientOptions, FixedIdentity, Identity, Policy, Text};
use ultar_launcher::service::{self, Admins};
use ultar_service::{Lifecycle, StopCause};

/// Patience for Wine: the prefix and a silent installer in a fresh cell.
const SETUP_WAIT: Duration = Duration::from_secs(900);

const IDENTITIES: [&str; 5] = [
    "system.launcher",
    "system.foreign",
    "system.sysdialog",
    "app.files",
    "app.settings",
];

fn shipped_policy() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../rootfs/etc/ultar/bus-policy.toml")
}

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

/// The system dialog: answers with `authorize`, keeps the questions.
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
    let deadline = Instant::now() + WAIT * 3;
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

/// The test's stand-in for the Wayland bridge: looks at the cell from inside, gives the
/// program an X server of its own (Xvfb, test only) and photographs its window once the
/// program has installed itself.
fn probe_script(world: &World) -> String {
    let secret = world.path("secret");
    format!(
        r#"#!/bin/sh
out="$HOME/probe.txt"
: >"$out"
r() {{ echo "$1 $2" >>"$out"; }}
r args "$*"
r uid "$(id -u)"
r net "$(tail -n +3 /proc/net/dev | cut -d: -f1 | tr -d ' ' | tr '\n' ' ')"
r setup "$(ls /.setup 2>/dev/null | tr '\n' ' ')"
if touch /.setup/ultar-probe 2>/dev/null; then r setup-write FAIL; else r setup-write refused; fi
Xvfb :7 -screen 0 1280x800x24 -nolisten tcp >/dev/null 2>&1 &
export DISPLAY=:7 ULTAR_WINE_DRIVER=x11 ULTAR_WINE_QUIET=1
sleep 2
shift
"$@" &
app=$!
i=0
while [ $i -lt 900 ] && [ ! -s "$HOME/.ultar-wine/program" ]; do sleep 1; i=$((i + 1)); done
r program "$(cat "$HOME/.ultar-wine/program" 2>/dev/null)"
sleep 25
xwd -root -display :7 -silent -out "$HOME/shot.xwd" 2>/dev/null
r drives "$(ls "$HOME/.wine/dosdevices" 2>/dev/null | tr '\n' ' ')"
export WINEPREFIX="$HOME/.wine" WINEDEBUG=-all
unset TMPDIR
# positive controls first: a Wine that does not run must not count as "refused"
if wine cmd /c 'dir C:\' 2>/dev/null | grep -qi 'windows'; then r c-drive ok; else r c-drive FAIL; fi
if wine cmd /c 'type \\?\unix\etc\os-release' 2>/dev/null | grep -q 'Ubuntu'; then r unix-cell ok; else r unix-cell FAIL; fi
if wine cmd /c 'dir Z:\' >/dev/null 2>&1; then r z FAIL; else r z refused; fi
if wine cmd /c 'type \\?\unix{secret}' 2>/dev/null | grep -q 'host secret'; then r unix-secret FAIL; else r unix-secret hidden; fi
r end ok
wait $app
"#,
        secret = secret.display()
    )
}

fn line<'a>(probe: &'a str, key: &str) -> &'a str {
    probe
        .lines()
        .find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix(' ')))
        .unwrap_or_else(|| panic!("{key} missing in\n{probe}"))
}

fn wait_file(path: &Path, text: &str, limit: Duration) -> String {
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
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Distinct pixel values in an XWD screenshot (Z pixmap, 32 bits per pixel); a window
/// with content has many, an empty screen one.
fn colours(xwd: &Path) -> usize {
    let b = std::fs::read(xwd).unwrap();
    let be = |at: usize| u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize;
    let (header, bpp, line, height, ncolors) = (be(0), be(44), be(48), be(20), be(76));
    assert_eq!(bpp, 32, "Xvfb at depth 24 writes 32 bits per pixel");
    let pixels = &b[header + ncolors * 12..];
    let mut seen = std::collections::BTreeSet::new();
    for row in pixels.chunks(line).take(height).step_by(4) {
        for px in row.chunks_exact(4).step_by(3) {
            seen.insert([px[0], px[1], px[2]]);
        }
    }
    seen.len()
}

struct Case<'a> {
    file: &'a str,
    source: Source,
    name: &'a str,
    kind: &'a str,
    program: &'a str,
}

#[test]
fn windows_programs_install_themselves_in_their_own_cells_and_leave_without_trace() {
    let vars = [
        "ULTAR_FOREIGN_RUNTIME",
        "ULTAR_FOREIGN_BIN",
        "ULTAR_WINDOWS_EXE",
        "ULTAR_WINDOWS_MSI",
        "ULTAR_WINDOWS_SHOTS",
    ]
    .map(std::env::var);
    let [Ok(area), Ok(bin), Ok(exe), Ok(msi), Ok(shots)] = vars else {
        eprintln!("skipped: ULTAR_FOREIGN_RUNTIME, ULTAR_FOREIGN_BIN, ULTAR_WINDOWS_EXE, ULTAR_WINDOWS_MSI, ULTAR_WINDOWS_SHOTS not set");
        return;
    };
    if !root() {
        eprintln!("skipped: real cells need root");
        return;
    }
    let mut world = World::new("winforeign");
    let foreign_root = world.path("foreign");
    let runtime = foreign_root.join("linux");
    std::fs::create_dir_all(&foreign_root).unwrap();
    assert!(Command::new("cp")
        .args(["-a", &area])
        .arg(&runtime)
        .status()
        .unwrap()
        .success());
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
    let exe_path = downloads.join(Path::new(&exe).file_name().unwrap());
    let msi_path = downloads.join(Path::new(&msi).file_name().unwrap());
    std::fs::copy(&exe, &exe_path).unwrap();
    std::fs::copy(&msi, &msi_path).unwrap();
    std::fs::write(world.path("secret"), b"host secret").unwrap();
    let tree = world.fake_group_tree();
    let before = std::fs::read_dir(runtime.join("var/lib/dpkg/info"))
        .unwrap()
        .count();

    let _broker = broker(&world);
    let config = format!(
        "{}foreign_runtime = \"{}\"\nforeign_bridge = \"{}\"\nforeign_setup = \"{}\"\n",
        world.config_text(&[], Some(&tree)),
        runtime.display(),
        bridge.display(),
        foreign_root.join("setup").display()
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
    let progress = files.subscribe::<Progress>().unwrap();
    assert!(common::wait_until_ok(|| files
        .call(
            &Inspect {
                path: "/nonexistent".into()
            },
            Duration::from_secs(1)
        )
        .is_ok()));
    let path = |p: &Path| p.display().to_string();
    let cases = [
        Case {
            file: &path(&exe_path),
            source: Source::Exe,
            name: "Notepad++",
            kind: "nsis",
            program: "notepad++",
        },
        Case {
            file: &path(&msi_path),
            source: Source::Msi,
            name: "7-Zip",
            kind: "msi",
            program: "7-zip file manager",
        },
    ];
    let mut installed = Vec::new();
    for (n, case) in cases.iter().enumerate() {
        // 1. described from its own data, unverified
        let info = match call(
            &files,
            &Inspect {
                path: case.file.into(),
            },
        )
        .outcome
        {
            InspectOutcome::Found(i) => i,
            other => panic!("{other:?}"),
        };
        assert_eq!(info.source, case.source);
        assert!(info.name.starts_with(case.name), "{info:?}");
        assert!(info.installable && !info.verified, "{info:?}");
        let install = Install {
            from: InstallFrom::File(case.file.into()),
            choices: Choices::default(),
        };
        // 2. cancelled: nothing
        authorize.store(false, Ordering::SeqCst);
        let job = started(call(&files, &install).outcome);
        assert_eq!(end_of(&progress, job).step, Step::Cancelled);
        assert!(launcher.list().iter().all(|a| !a
            .app
            .starts_with(&format!("foreign.{}", case.source.word()))));
        assert_eq!(
            asked.lock().unwrap().last().unwrap().args[1],
            ultar_bus::TextArg::Key(format!("foreign.source.{}", case.source.word()))
        );
        // 3. confirmed: the app with its own installer
        authorize.store(true, Ordering::SeqCst);
        let job = started(call(&files, &install).outcome);
        let done = end_of(&progress, job);
        assert_eq!(done.step, Step::Done, "{done:?}");
        let id = done.apps[0].clone();
        assert!(
            id.starts_with(&format!("foreign.{}-", case.source.word())),
            "{id}"
        );
        let app = launcher.list().into_iter().find(|a| a.app == id).unwrap();
        assert_eq!(app.profile, "app.foreign");
        let setup = foreign_root.join("setup").join(&id);
        assert_eq!(std::fs::read_dir(&setup).unwrap().count(), 1);
        println!(
            "FOREIGN-WINDOWS: {} registered as {id} (uid {})",
            case.name, app.uid
        );

        // the fake group tree has no controller that makes the app's group files
        let group = tree.join("apps").join(&id);
        std::fs::create_dir_all(&group).unwrap();
        for file in ["cgroup.procs", "memory.max", "pids.max"] {
            std::fs::write(group.join(file), b"").unwrap();
        }

        // 4. first start: installs itself in its own cell, its window shows
        launcher.start(&id, Role::Ui, None).unwrap();
        let probe = wait_file(
            &world.path(&format!("appdata/{id}/probe.txt")),
            "end ok",
            SETUP_WAIT,
        );
        let args = line(&probe, "args");
        assert!(
            args.starts_with(&format!(
                "-- /usr/lib/ultar/wine/ultar-wine-app {} ",
                case.kind
            )),
            "{probe}"
        );
        assert_eq!(line(&probe, "uid"), app.uid.to_string());
        assert_eq!(
            line(&probe, "net").trim(),
            "lo",
            "no network without the permission"
        );
        assert_eq!(
            line(&probe, "setup-write"),
            "refused",
            "the installer is read-only"
        );
        let program = line(&probe, "program").to_lowercase();
        assert!(program.contains(case.program), "{probe}");
        assert_eq!(line(&probe, "drives").trim(), "c:", "only C: – {probe}");
        // Wine runs in the cell and reaches what the cell sees, so the refusals below are real
        assert_eq!(line(&probe, "c-drive"), "ok", "{probe}");
        assert_eq!(line(&probe, "unix-cell"), "ok", "{probe}");
        assert_eq!(line(&probe, "z"), "refused");
        assert_eq!(line(&probe, "unix-secret"), "hidden");
        let shot = world.path(&format!("appdata/{id}/shot.xwd"));
        let kept = Path::new(&shots).join(format!("{n}-{}.xwd", case.source.word()));
        std::fs::copy(&shot, &kept).unwrap();
        let seen = colours(&shot);
        assert!(
            seen > 40,
            "the program's window shows ({seen} colours, {})",
            kept.display()
        );
        let _ = launcher.stop(&id, Role::Ui);
        println!(
            "FOREIGN-WINDOWS: {} installed itself and runs in its cell ({seen} colours, {})",
            case.name,
            kept.display()
        );
        installed.push(id);
    }

    // 5. removal (Settings) leaves nothing behind
    let settings = client(&world, "app.settings");
    let listed = call(&settings, &List {}).apps;
    assert_eq!(listed.len(), 2);
    for id in &installed {
        let icon = listed.iter().find(|a| &a.id == id).unwrap().icon.clone();
        let removed = call(
            &settings,
            &Remove {
                id: id.clone(),
                keep_data: false,
            },
        );
        assert_eq!(removed.outcome, Outcome::Done, "{id}");
        assert!(common::wait_until_ok(
            || !call(&settings, &RuntimeState {}).busy
        ));
        assert!(
            !world.path(&format!("appdata/{id}")).exists(),
            "data of {id}"
        );
        assert!(
            !foreign_root.join("setup").join(id).exists(),
            "installer of {id}"
        );
        assert!(
            icon.is_empty() || !Path::new(&icon).exists(),
            "icon of {id}"
        );
    }
    assert!(call(&settings, &List {}).apps.is_empty());
    assert!(launcher
        .list()
        .iter()
        .all(|a| !a.app.starts_with("foreign.")));
    assert_eq!(
        std::fs::read_dir(runtime.join("var/lib/dpkg/info"))
            .unwrap()
            .count(),
        before,
        "the Linux area is unchanged"
    );
    println!("FOREIGN-WINDOWS: removal ok");

    let _ = Command::new("kill")
        .args(["-TERM", &service.id().to_string()])
        .status();
    assert_eq!(service.wait().unwrap().code(), Some(0));
    stop.store(true, Ordering::SeqCst);
    life.request_stop(StopCause::Request);
    drop(_broker);
    serving.join().unwrap();
    println!("FOREIGN-WINDOWS-E2E: ALLES OK");
}
