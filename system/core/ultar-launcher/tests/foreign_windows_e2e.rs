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
mod foreign;

use common::{root, World, WAIT};
use foreign::{call, client, end_of, line, started, text, wait_file, Device};
use std::path::{Path, PathBuf};
use std::time::Duration;
use ultar_bus::messages::foreign::{
    Choices, Inspect, InspectOutcome, Install, InstallFrom, List, Outcome, Progress, Remove,
    RuntimeState, Source, Step,
};
use ultar_bus::messages::launcher::Role;
use ultar_bus::Client;

/// Patience for Wine: the prefix and a silent installer in a fresh cell.
const SETUP_WAIT: Duration = Duration::from_secs(900);

const IDENTITIES: [&str; 5] = [
    "system.launcher",
    "system.foreign",
    "system.sysdialog",
    "app.files",
    "app.settings",
];

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

/// Distinct pixel values in an XWD screenshot (Z pixmap, 24 or 32 bits per pixel – Xvfb
/// versions differ at depth 24); a window with content has many, an empty screen one.
fn colours(xwd: &Path) -> usize {
    let b = std::fs::read(xwd).unwrap();
    let be = |at: usize| u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize;
    let (header, bpp, line, height, ncolors) = (be(0), be(44), be(48), be(20), be(76));
    assert!(bpp == 24 || bpp == 32, "XWD with {bpp} bits per pixel");
    let pixels = &b[header + ncolors * 12..];
    let mut seen = std::collections::BTreeSet::new();
    for row in pixels.chunks(line).take(height).step_by(4) {
        for px in row.chunks_exact(bpp / 8).step_by(3) {
            seen.insert([px[0], px[1], px[2]]);
        }
    }
    seen.len()
}

struct Case {
    file: String,
    source: Source,
    name: &'static str,
    kind: &'static str,
    program: &'static str,
}

/// What the steps share: the world, the running device, Files and the places.
struct Scene {
    world: World,
    device: Device,
    files: Client,
    progress: ultar_bus::TypedSubscription<Progress>,
    foreign_root: PathBuf,
    runtime: PathBuf,
    tree: PathBuf,
    /// Where the screenshots go.
    shots: PathBuf,
}

/// The area (a copy), the bridge, the person's two installers, a secret of the host; the
/// launcher's configuration and the two cases.
fn prepare(world: &World, area: &str, exe: &str, msi: &str) -> (String, [Case; 2]) {
    let foreign_root = world.path("foreign");
    let runtime = foreign_root.join("linux");
    foreign::copy_area(area, &runtime);
    let bridge = foreign::fake_bridge(world, &probe_script(world));
    let downloads = world.path("areas/downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let exe_path = downloads.join(Path::new(exe).file_name().unwrap());
    let msi_path = downloads.join(Path::new(msi).file_name().unwrap());
    std::fs::copy(exe, &exe_path).unwrap();
    std::fs::copy(msi, &msi_path).unwrap();
    std::fs::write(world.path("secret"), b"host secret").unwrap();
    let tree = world.fake_group_tree();
    let config = format!(
        "{}foreign_runtime = \"{}\"\nforeign_bridge = \"{}\"\nforeign_setup = \"{}\"\n",
        world.config_text(&[], Some(&tree)),
        runtime.display(),
        bridge.display(),
        foreign_root.join("setup").display()
    );
    let cases = [
        Case {
            file: text(&exe_path),
            source: Source::Exe,
            name: "Notepad++",
            kind: "nsis",
            program: "notepad++",
        },
        Case {
            file: text(&msi_path),
            source: Source::Msi,
            name: "7-Zip",
            kind: "msi",
            program: "7-zip file manager",
        },
    ];
    (config, cases)
}

/// The packages of the Linux area.
fn packages(runtime: &Path) -> usize {
    std::fs::read_dir(runtime.join("var/lib/dpkg/info"))
        .unwrap()
        .count()
}

/// The device with the service, once it answers.
fn start(mut world: World, config: &str, bin: &str, shots: &str) -> Scene {
    let foreign_root = world.path("foreign");
    let device = Device::start(&mut world, &IDENTITIES, config, bin, &foreign_root);
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
    Scene {
        runtime: foreign_root.join("linux"),
        foreign_root,
        tree: world.path("groups"),
        world,
        device,
        files,
        progress,
        shots: PathBuf::from(shots),
    }
}

/// 1. described from its own data, unverified; 2. cancelled: nothing.
fn described_and_cancelled(s: &Scene, case: &Case) -> Install {
    let info = match call(
        &s.files,
        &Inspect {
            path: case.file.clone(),
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
        from: InstallFrom::File(case.file.clone()),
        choices: Choices::default(),
    };
    s.device.authorize(false);
    let job = started(call(&s.files, &install).outcome);
    assert_eq!(end_of(&s.progress, job, WAIT * 3).step, Step::Cancelled);
    let prefix = format!("foreign.{}", case.source.word());
    assert!(s
        .device
        .launcher
        .list()
        .iter()
        .all(|a| !a.app.starts_with(&prefix)));
    assert_eq!(
        s.device.dialog.asked.lock().unwrap().last().unwrap().args[1],
        ultar_bus::TextArg::Key(format!("foreign.source.{}", case.source.word()))
    );
    install
}

/// 3. confirmed: the app with its own installer; its id and user id.
fn confirmed(s: &Scene, case: &Case, install: &Install) -> (String, u32) {
    s.device.authorize(true);
    let job = started(call(&s.files, install).outcome);
    let done = end_of(&s.progress, job, WAIT * 3);
    assert_eq!(done.step, Step::Done, "{done:?}");
    let id = done.apps[0].clone();
    assert!(
        id.starts_with(&format!("foreign.{}-", case.source.word())),
        "{id}"
    );
    let app = s
        .device
        .launcher
        .list()
        .into_iter()
        .find(|a| a.app == id)
        .unwrap();
    assert_eq!(app.profile, "app.foreign");
    let setup = s.foreign_root.join("setup").join(&id);
    assert_eq!(std::fs::read_dir(&setup).unwrap().count(), 1);
    println!(
        "FOREIGN-WINDOWS: {} registered as {id} (uid {})",
        case.name, app.uid
    );
    foreign::group_files(&s.tree, &id);
    (id, app.uid)
}

/// 4. what the program sees in its cell once it installed itself.
fn cell_probe(probe: &str, case: &Case, uid: u32) {
    let args = line(probe, "args");
    assert!(
        args.starts_with(&format!(
            "-- /usr/lib/ultar/wine/ultar-wine-app {} ",
            case.kind
        )),
        "{probe}"
    );
    assert_eq!(line(probe, "uid"), uid.to_string());
    assert_eq!(
        line(probe, "net").trim(),
        "lo",
        "no network without the permission"
    );
    assert_eq!(
        line(probe, "setup-write"),
        "refused",
        "the installer is read-only"
    );
    let program = line(probe, "program").to_lowercase();
    assert!(program.contains(case.program), "{probe}");
    assert_eq!(line(probe, "drives").trim(), "c:", "only C: – {probe}");
    // Wine runs in the cell and reaches what the cell sees, so the refusals below are real
    assert_eq!(line(probe, "c-drive"), "ok", "{probe}");
    assert_eq!(line(probe, "unix-cell"), "ok", "{probe}");
    assert_eq!(line(probe, "z"), "refused");
    assert_eq!(line(probe, "unix-secret"), "hidden");
}

/// 4. first start: installs itself in its own cell, its window shows.
fn first_start(s: &Scene, case: &Case, n: usize, (id, uid): (&str, u32)) {
    s.device.launcher.start(id, Role::Ui, None).unwrap();
    let probe = wait_file(
        &s.world.path(&format!("appdata/{id}/probe.txt")),
        "end ok",
        SETUP_WAIT,
    );
    cell_probe(&probe, case, uid);
    let shot = s.world.path(&format!("appdata/{id}/shot.xwd"));
    let kept = s.shots.join(format!("{n}-{}.xwd", case.source.word()));
    std::fs::copy(&shot, &kept).unwrap();
    let seen = colours(&shot);
    assert!(
        seen > 40,
        "the program's window shows ({seen} colours, {})",
        kept.display()
    );
    let _ = s.device.launcher.stop(id, Role::Ui);
    println!(
        "FOREIGN-WINDOWS: {} installed itself and runs in its cell ({seen} colours, {})",
        case.name,
        kept.display()
    );
}

/// 5. removal (Settings) leaves nothing behind.
fn removal(s: &Scene, installed: &[String], before: usize) {
    let settings = client(&s.world, "app.settings");
    let listed = call(&settings, &List {}).apps;
    assert_eq!(listed.len(), 2);
    for id in installed {
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
        let data = s.world.path(&format!("appdata/{id}"));
        assert!(!data.exists(), "data of {id}");
        let setup = s.foreign_root.join("setup").join(id);
        assert!(!setup.exists(), "installer of {id}");
        assert!(
            icon.is_empty() || !Path::new(&icon).exists(),
            "icon of {id}"
        );
    }
    assert!(call(&settings, &List {}).apps.is_empty());
    assert!(s
        .device
        .launcher
        .list()
        .iter()
        .all(|a| !a.app.starts_with("foreign.")));
    assert_eq!(packages(&s.runtime), before, "the Linux area is unchanged");
    println!("FOREIGN-WINDOWS: removal ok");
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
    let world = World::new("winforeign");
    let (config, cases) = prepare(&world, &area, &exe, &msi);
    let before = packages(&world.path("foreign/linux"));
    let scene = start(world, &config, &bin, &shots);
    let mut installed = Vec::new();
    for (n, case) in cases.iter().enumerate() {
        let install = described_and_cancelled(&scene, case);
        let (id, uid) = confirmed(&scene, case, &install);
        first_start(&scene, case, n, (&id, uid));
        installed.push(id);
    }
    removal(&scene, &installed, before);
    let Scene { device, .. } = scene;
    device.stop();
    println!("FOREIGN-WINDOWS-E2E: ALLES OK");
}
