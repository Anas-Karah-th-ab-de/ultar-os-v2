//! A program from another system the way a person gets it, on the Ultar kernel
//! (`tools/kern-test/foreign-app-kern.sh`): the bus with the shipped policy, the
//! launcher, the system dialog (answers "authorized"), the compositor on the screen and
//! the real service `ultar-foreign`. Files installs `mousepad.deb` through
//! `foreign.install`; the app is started through the launcher as from the app grid – in
//! its cell (`app.foreign`, own user, the Linux area read-only, behind `ultar-wayland`,
//! its own title bar through `ui.client-header`); the QEMU driver takes the screenshot at
//! `FOREIGN-APP: SHOT`; the stop ends the cell; removal leaves nothing behind.
//! Last line: `FOREIGN-APP-KERN: ALLES OK` or `FOREIGN-APP-KERN: FEHLER <what>`.
//!
//! ```text
//! foreignapp --bin DIR --deb FILE [--groups] [--draw SECONDS] [--component PROGRAM]
//! ```
//!
//! Runs as root. `DIR` holds the static `ultar-launcher`, `ultar-foreign`,
//! `ultar-compositor` and `ultar-wayland`; the data disk holds the Linux area at
//! `/data/foreign/linux`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod world;

use std::path::Path;
use std::time::Duration;
use ultar_bus::messages::foreign::{
    Choices, Install, InstallFrom, InstallOutcome, List, Outcome, Remove, RuntimeState, Step,
};
use ultar_bus::messages::launcher::Role;
use world::{Args, World, APP};

/// Patience for the package manager (QEMU without KVM is slow).
const APT_WAIT: Duration = Duration::from_secs(1500);

fn fail(what: &str) -> ! {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "FOREIGN-APP-KERN: FEHLER {what}");
    let _ = out.flush();
    drop(out);
    // the console of the kernel is slow: let the line out before the process ends
    std::thread::sleep(Duration::from_secs(2));
    std::process::exit(1);
}

fn check(ok: bool, what: &str) {
    if !ok {
        fail(what);
    }
}

/// With `--component PROGRAM` (ultar-foreign's `componenttest`): the component „Linux-Programme“
/// as the service installs it, on this kernel, before the app (its lines pass through).
fn component_first() {
    let words: Vec<String> = std::env::args().collect();
    let arg = |name: &str| words.iter().position(|w| w == name).and_then(|i| words.get(i + 1)).cloned();
    let (Some(test), Some(bin)) = (arg("--component"), arg("--bin")) else {
        return;
    };
    let out = std::process::Command::new(&test)
        // below the service's root, where the kernel test's data disk takes new directories
        .args(["--helper", &format!("{bin}/ultar-foreign"), "--work", "/data/foreign/componenttest"])
        .output()
        .unwrap_or_else(|e| fail(&format!("component: {e}")));
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines().chain(String::from_utf8_lossy(&out.stderr).lines().take(20)) {
        println!("{line}");
    }
    // the app's own way is checked whatever the component test says (the script needs both)
    if !text.contains("COMPONENT: ALLES OK") {
        println!("COMPONENT: FEHLER (see above)");
    }
}

fn main() {
    let args = Args::parse().unwrap_or_else(|| {
        println!("foreignapp --bin DIR --deb FILE [--groups] [--draw SECONDS]");
        std::process::exit(2)
    });
    component_first();
    let world = World::start(&args).unwrap_or_else(|e| fail(&format!("setup: {e}")));
    let files = world.client("app.files");
    let settings = world.client("app.settings");
    let progress = files
        .subscribe::<ultar_bus::messages::foreign::Progress>()
        .unwrap();
    // the first answer counts the Linux area once (slow on the Ultar kernel)
    let mut last = String::from("no answer");
    let seen = world::poll(Duration::from_secs(600), || {
        match settings.call(&RuntimeState {}, Duration::from_secs(120)) {
            Ok(r) if r.installed => true,
            Ok(r) => {
                last = format!("{r:?}");
                false
            }
            Err(e) => {
                last = format!("{e:?}");
                false
            }
        }
    });
    check(seen, &format!("the service does not see the Linux area: {last}"));
    println!("FOREIGN-APP: service ok");

    // 1. Files installs the package of the person (the dialog answers "authorized")
    let install = Install {
        from: InstallFrom::File(world.deb.display().to_string()),
        choices: Choices::default(),
    };
    let InstallOutcome::Started(job) = world.call(&files, &install).outcome else {
        fail("install did not start");
    };
    let done = world::end_of(&progress, job, APT_WAIT).unwrap_or_else(|| fail("install did not end"));
    check(done.step == Step::Done, &format!("install: {done:?}"));
    check(done.apps == [APP.to_owned()], &format!("apps: {:?}", done.apps));
    check(world.asked() >= 1, "the administrator was not asked");
    let status = world.app().unwrap_or_else(|| fail("no launcher entry"));
    check(status.profile == "app.foreign", &status.profile);
    check(status.uid >= 20000, "the app has no own user");
    let uid = status.uid;
    println!("FOREIGN-APP: installed uid {uid}");
    let rights = std::fs::read_to_string(world.compositor_rights()).unwrap_or_default();
    check(
        rights
            .lines()
            .any(|l| l.starts_with(&format!("{uid} ")) && l.contains("ui.client-header")),
        "no ui.client-header for the app",
    );
    println!("FOREIGN-APP: rights ok");

    // 2. started as from the app grid: its cell, behind the bridge, on the screen
    world
        .launcher
        .start(APP, Role::Ui, None)
        .unwrap_or_else(|e| fail(&format!("start: {e:?}")));
    check(
        world::poll(Duration::from_secs(600), || !world.cell_pids().is_empty()),
        "the cell has no process",
    );
    check(
        world::poll(Duration::from_secs(600), || world.window_drawn()),
        "no window of the app",
    );
    println!("FOREIGN-APP: running in apps/{APP} ({} processes)", world.cell_pids().len());
    println!("FOREIGN-APP: SHOT");
    std::thread::sleep(Duration::from_secs(args.draw));
    let owned = world.data_owned_by(uid);
    println!("FOREIGN-APP: files in its data owned by {uid}: {owned}");

    // 3. the stop ends the cell
    let _ = world.launcher.stop(APP, Role::Ui);
    check(
        world::poll(Duration::from_secs(120), || world.cell_pids().is_empty()),
        "the cell outlived the stop",
    );
    println!("FOREIGN-APP: stopped");

    // 4. removal leaves nothing behind
    let removed = world.call(
        &settings,
        &Remove {
            id: APP.into(),
            keep_data: false,
        },
    );
    check(removed.outcome == Outcome::Done, &format!("remove: {:?}", removed.outcome));
    check(
        world::poll(APT_WAIT, || {
            !world.call(&settings, &RuntimeState {}).busy && !world.installed("mousepad")
        }),
        "mousepad stayed in the area",
    );
    check(world.app().is_none(), "the launcher entry stayed");
    check(world.call(&settings, &List {}).apps.is_empty(), "the service lists it");
    check(!world.app_data().exists(), "its data stayed");
    check(!Path::new("/data/foreign/linux/usr/bin/mousepad").exists(), "the program stayed");
    println!("FOREIGN-APP: removed");
    world.stop();
    println!("FOREIGN-APP-KERN: ALLES OK");
}
