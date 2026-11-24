//! Reviewer attack tests (round 1). Each test states the property the
//! launcher should have; a failing test is a finding of the review report
//! (`launcher-runde-1.md`).

mod common;

use common::{root, wait_state, Collector, World};
use ultar_bus::messages::launcher::{InstanceState as S, Role};

/// F1: an update of an installed app must come from the same signer (or at
/// least the same profile). Otherwise any store-signed package that names
/// the id of a system app takes over its user id and its data directory.
#[test]
fn a_store_package_cannot_take_over_a_system_apps_id() {
    let mut world = World::new("hijack");
    let launcher = world.open_default();
    let system = launcher
        .install(&world.package("good", "1.0.0", "", true))
        .unwrap();
    assert_eq!(system.profile, "app.system");
    std::fs::write(world.path("appdata/good/secret"), b"system app's secret").unwrap();
    let store = launcher.install(&world.package("good", "2.0.0", "", false));
    assert!(
        store.is_err(),
        "store-signed package replaced the system app 'good' and got its uid/data: {store:?}"
    );
}

/// F1b: the same through uninstall with `keep_data`: the id, uid and the
/// kept data go to whoever installs a package with that id next.
#[test]
fn kept_data_does_not_go_to_another_publisher() {
    let mut world = World::new("hijack2");
    let launcher = world.open_default();
    let first = launcher
        .install(&world.package("good", "1.0.0", "", true))
        .unwrap();
    std::fs::write(world.path("appdata/good/secret"), b"secret").unwrap();
    launcher.uninstall("good", true).unwrap();
    let second = launcher.install(&world.package("good", "1.0.0", "", false));
    if let Ok(reply) = &second {
        assert!(
            reply.uid != first.uid || !world.path("appdata/good/secret").exists(),
            "a store package got uid {} and the kept data of the system app",
            reply.uid
        );
    }
}

/// F2: `/data` is mounted noexec ("nothing may be executed from it"), but
/// the private `/tmp` is not: an app can still run code it wrote itself.
#[test]
fn an_app_cannot_execute_what_it_wrote() {
    if !root() {
        eprintln!("skipped: needs real cells (root)");
        return;
    }
    let mut world = World::new("tmpexec");
    let collector = Collector::listen(&world.path("out.sock"));
    let launcher = world.open_default();
    let text = "id = \"evil\"\nversion = \"1.0.0\"\nkind = \"native\"\nname = \"app.evil.name\"\n\
                exec = \"/bin/sh\"\nargs = [\"-c\", \"cp /bin/true /data/t 2>/dev/null; /data/t && echo DATA-EXEC-OK; \
                cp /bin/true /tmp/t && /tmp/t && echo TMP-EXEC-OK; echo DONE\"]\n";
    launcher
        .install(&world.sign("evil-sh", text, true))
        .unwrap();
    launcher.start("evil", Role::Ui, None).unwrap();
    wait_state(&launcher, "evil", Role::Ui, &[S::Stopped, S::Crashed]);
    assert!(collector.wait_for("app-evil", "DONE").is_some());
    let lines = collector.of("app-evil");
    assert!(
        !lines.iter().any(|l| l.contains("DATA-EXEC-OK")),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains("TMP-EXEC-OK")),
        "app executed a program it wrote to /tmp: {lines:?}"
    );
}

/// F3: what the app sees of the launcher's descriptors: only 0, 1, 2.
#[test]
fn no_launcher_descriptor_reaches_the_app() {
    if !root() {
        eprintln!("skipped: needs real cells (root)");
        return;
    }
    let mut world = World::new("fds");
    let collector = Collector::listen(&world.path("out.sock"));
    let launcher = world.open_default();
    let text = "id = \"evil\"\nversion = \"1.0.0\"\nkind = \"native\"\nname = \"app.evil.name\"\n\
                exec = \"/bin/sh\"\nargs = [\"-c\", \"echo FDS $(ls /proc/self/fd | tr '\\\\n' ' '); echo PID1 $(cat /proc/1/status | grep -E '^(Uid|CapEff)' | tr '\\\\n' ' '); echo P1FD $(ls /proc/1/fd 2>&1 | tr '\\\\n' ' '); echo 'EXIT 0' > /proc/1/fd/3 2>/dev/null && echo STATUS-WRITTEN; echo END\"]\n";
    launcher
        .install(&world.sign("evil-fd", text, true))
        .unwrap();
    launcher.start("evil", Role::Ui, None).unwrap();
    wait_state(&launcher, "evil", Role::Ui, &[S::Stopped, S::Crashed]);
    let fds = collector.wait_for("app-evil", "FDS").unwrap();
    eprintln!("{fds}\n{:?}", collector.wait_for("app-evil", "PID1"));
    // ls itself opens one directory descriptor (3)
    let n: Vec<&str> = fds.split_whitespace().skip(1).collect();
    assert!(n.len() <= 4, "descriptors in the app: {fds}");
    // pid 1 of the cell runs as the app's user and holds the status pipe: the app must not reach it
    collector.wait_for("app-evil", "END").unwrap();
    let lines = collector.of("app-evil");
    eprintln!("{lines:?}");
    assert!(
        !lines.iter().any(|l| l == "STATUS-WRITTEN"),
        "app wrote to the launcher's status pipe: {lines:?}"
    );
}

/// F4: a revoke must stop the app and take the right away even if the
/// compositor file cannot be written (fail closed). Simulated: the rights
/// file's directory is made unwritable for the launcher (root ignores the
/// mode, so a file in its place makes the rename fail).
#[test]
fn a_revoke_takes_effect_even_if_the_rights_file_cannot_be_written() {
    let mut world = World::new("revoke");
    let launcher = world.open_default();
    let extra = "[permissions]\nui = [\"ui.capture\"]\n";
    launcher
        .install(&world.package("good", "1.0.0", extra, true))
        .unwrap();
    launcher.start("good", Role::Ui, None).unwrap();
    wait_state(&launcher, "good", Role::Ui, &[S::Running]);
    // a directory where the file goes: the atomic rename fails
    let file = world.path("share/compositor-apps.toml");
    let _ = std::fs::remove_file(&file);
    std::fs::create_dir(&file).unwrap();
    std::fs::write(file.join("x"), b"x").unwrap();
    let r = launcher.set_permission("good", "ui.capture", false);
    let state = launcher
        .list()
        .into_iter()
        .find(|a| a.app == "good")
        .map(|a| a.instances[0].state);
    assert!(
        r.is_ok() || matches!(state, Some(S::Stopping | S::Stopped)),
        "revoke failed ({r:?}) and the app still runs ({state:?}) with the right on file"
    );
}
