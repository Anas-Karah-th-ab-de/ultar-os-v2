//! The findings of review round 1 (`launcher-runde-1.md`), each as the
//! property that now holds.

mod common;

use common::{root, wait_state, Collector, World};
use std::time::{Duration, Instant};
use ultar_bus::messages::launcher::{InstanceState as S, Role};
use ultar_launcher::error::{LaunchError, PackageError};

fn package_error(
    result: Result<ultar_bus::messages::launcher::InstallReply, LaunchError>,
) -> PackageError {
    match result {
        Err(LaunchError::Package(error)) => error,
        other => panic!("not refused as a package: {other:?}"),
    }
}

/// L-1: an app is its id and its signer.
#[test]
fn an_app_is_its_id_and_its_signer() {
    let mut world = World::new("signer");
    let launcher = world.open_default();
    let first = launcher
        .install(&world.package("good", "1.4.0", "", true))
        .unwrap();
    std::fs::write(world.path("appdata/good/secret"), b"s").unwrap();
    // another system key, same id: refused (installed, and after an uninstall with kept data)
    let km2 = world.another_key("keys/system", "km2");
    let other_signer =
        world.sign_with("good-km2", &World::manifest_text("good", "2.0.0", ""), &km2);
    assert_eq!(
        package_error(launcher.install(&other_signer)),
        PackageError::Signer
    );
    // a lower version of the same signer: refused; the same or a higher one: fine
    assert_eq!(
        package_error(launcher.install(&world.package("good", "1.3.9", "", true))),
        PackageError::Downgrade
    );
    assert_eq!(
        launcher
            .install(&world.package("good", "1.4.0", "", true))
            .unwrap()
            .uid,
        first.uid
    );
    launcher.uninstall("good", true).unwrap();
    assert_eq!(
        package_error(launcher.install(&other_signer)),
        PackageError::Signer
    );
    assert!(world.path("appdata/good/secret").exists());
    // the same signer gets its id and its kept data back
    let again = launcher
        .install(&world.package("good", "1.5.0", "", true))
        .unwrap();
    assert_eq!(again.uid, first.uid);
    assert_eq!(
        std::fs::read(world.path("appdata/good/secret")).unwrap(),
        b"s"
    );
}

/// L-1: store ids carry the publisher of their key; system ids none.
#[test]
fn store_ids_carry_their_publisher() {
    let mut world = World::new("publisher");
    let launcher = world.open_default();
    // the store key of the world is `store.ed25519`: its apps are `store.*`
    assert!(launcher
        .install(&world.package("store.game", "1.0.0", "", false))
        .is_ok());
    for (id, extra) in [
        ("game", ""),
        ("acme.game", ""),
        ("store.app", "publisher = \"acme\"\n"),
    ] {
        let text = World::manifest_text(id, "1.0.0", extra);
        let package = world.sign(&format!("p-{id}"), &text, false);
        assert_eq!(
            package_error(launcher.install(&package)),
            PackageError::Publisher,
            "{id}"
        );
    }
    // a second publisher cannot name the first one's apps
    let acme = world.another_key("keys", "acme");
    let text = World::manifest_text("store.game", "9.0.0", "");
    let package = world.sign_with("acme-takes-store", &text, &acme);
    assert_eq!(
        package_error(launcher.install(&package)),
        PackageError::Publisher
    );
    let own = world.sign_with(
        "acme-own",
        &World::manifest_text("acme.game", "1.0.0", ""),
        &acme,
    );
    assert_eq!(launcher.install(&own).unwrap().profile, "app.standard");
    // a system package cannot use a publisher id
    let dotted = world.sign(
        "km-dotted",
        &World::manifest_text("km.notes", "1.0.0", ""),
        true,
    );
    assert_eq!(
        package_error(launcher.install(&dotted)),
        PackageError::Publisher
    );
}

/// L-4: a revoke stops the app even if the compositor's file cannot be
/// written, and the old file is withdrawn (no app keeps a right on file).
#[test]
fn a_revoke_withdraws_the_rights_file_when_it_cannot_be_written() {
    let mut world = World::new("withdraw");
    let launcher = world.open_default();
    let extra = "[permissions]\nui = [\"ui.capture\"]\n";
    launcher
        .install(&world.package("good", "1.0.0", extra, true))
        .unwrap();
    let file = world.path("share/compositor-apps.toml");
    assert!(std::fs::read_to_string(&file)
        .unwrap()
        .contains("20000 = \"ui.capture\""));
    launcher.start("good", Role::Ui, None).unwrap();
    // the temporary name is taken by a directory: the atomic write fails
    std::fs::create_dir(world.path("share/.compositor-apps.toml.new")).unwrap();
    std::fs::write(world.path("share/.compositor-apps.toml.new/x"), b"x").unwrap();
    let revoked = launcher.set_permission("good", "ui.capture", false);
    assert!(
        matches!(revoked, Err(LaunchError::Storage(_))),
        "{revoked:?}"
    );
    assert!(!file.exists(), "the old file with the right is withdrawn");
    wait_state(&launcher, "good", Role::Ui, &[S::Stopped]);
    // a grant that cannot reach the compositor is taken back
    assert!(launcher.set_permission("good", "ui.capture", true).is_err());
    assert!(!launcher.permissions("good").unwrap().declared[0].granted);
}

/// L-7: uninstalling waits until the cell ended, then removes the data.
#[test]
fn uninstall_ends_the_app_before_its_data_goes() {
    let mut world = World::new("uninstall");
    let launcher = world.open_default();
    launcher
        .install(&world.package("good", "1.0.0", "", true))
        .unwrap();
    launcher.start("good", Role::Ui, None).unwrap();
    assert_eq!(launcher.uninstall("good", false), Ok(1));
    assert!(!world.path("appdata/good").exists());
    assert!(launcher.list().is_empty());
    assert_eq!(
        launcher.start("good", Role::Ui, None),
        Err(LaunchError::UnknownApp)
    );
}

fn alive(pid: u32) -> bool {
    std::path::Path::new(ultar_sys::cell::PROC)
        .join(pid.to_string())
        .exists()
}

/// L-3: cells of a launcher that died are ended when it starts again (found
/// by their memory group): no second copy of an app.
#[test]
fn a_restarted_launcher_ends_the_cells_of_its_previous_run() {
    if !root() {
        eprintln!("skipped: real cells need root (cleanup.rs tests the group part)");
        return;
    }
    let mut world = World::new("leftover");
    let tree = world.fake_group_tree();
    let config = world.config_text(&[], Some(&tree));
    let launcher = world.open(&config);
    launcher
        .install(&world.package("good", "1.0.0", "", true))
        .unwrap();
    let old = launcher.start("good", Role::Ui, None).unwrap().pid;
    assert!(alive(old));
    world.abandon();
    let launcher = world.open(&config);
    let deadline = Instant::now() + common::WAIT;
    while alive(old) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive(old), "the cell of the previous run still runs");
    let new = launcher.start("good", Role::Ui, None).unwrap();
    assert!(!new.already);
    let again = launcher.start("good", Role::Ui, None).unwrap();
    assert!(again.already && again.pid == new.pid, "single instance");
}

/// L-5: the launcher never signals a number it no longer owns: after the
/// app's cell was ended from outside, a new process that got the cell's
/// (and stage 1's) old numbers survives stop and uninstall.
#[test]
fn a_reused_process_number_is_never_signalled() {
    if !root() {
        eprintln!("skipped: needs root (ns_last_pid, real cells)");
        return;
    }
    let mut world = World::new("reuse");
    let launcher = world.open_default();
    launcher
        .install(&world.package("good", "1.0.0", "", true))
        .unwrap();
    let cell = launcher.start("good", Role::Ui, None).unwrap().pid;
    ultar_sys::cell::send_signal(cell, ultar_sys::cell::StopSignal::Kill).unwrap();
    wait_state(&launcher, "good", Role::Ui, &[S::Crashed, S::Stopped]);
    let last = format!("{}/sys/kernel/ns_last_pid", ultar_sys::cell::PROC);
    let mut victim = None;
    for _ in 0..20 {
        let _ = std::fs::write(&last, (cell - 1).to_string());
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        if child.id() == cell {
            victim = Some(child);
            break;
        }
        let mut other = child;
        let _ = other.kill();
        let _ = other.wait();
    }
    let Some(mut victim) = victim else {
        eprintln!("skipped: the kernel did not give the number again");
        return;
    };
    let _ = launcher.stop("good", Role::Ui);
    let _ = launcher.uninstall("good", false);
    assert!(
        victim.try_wait().unwrap().is_none(),
        "the new owner of the number was signalled"
    );
    let _ = victim.kill();
    let _ = victim.wait();
}

/// L-11: without a pid namespace (the operator accepted that) an app still
/// cannot signal another app: they are different users.
#[test]
fn an_app_cannot_signal_another_apps_process_without_a_pid_namespace() {
    if !root() {
        eprintln!("skipped: needs own user ids (root)");
        return;
    }
    let mut world = World::new("nopidns");
    let collector = Collector::listen(&world.path("out.sock"));
    let tree = world.fake_group_tree();
    let config = world.config_text(&["cell", "net"], Some(&tree));
    let launcher = world.open(&config);
    launcher
        .install(&world.package("other", "1.0.0", "", true))
        .unwrap();
    let victim = launcher.start("other", Role::Ui, None).unwrap().pid;
    let args = ["attack", "--victim-pid", &victim.to_string()].map(str::to_owned);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    launcher
        .install(&world.package_args("store.evil", &args, "", false))
        .unwrap();
    launcher.start("store.evil", Role::Ui, None).unwrap();
    wait_state(&launcher, "store.evil", Role::Ui, &[S::Stopped, S::Crashed]);
    assert!(collector
        .wait_for("app-store_evil", "ATTACK kill_other_app blocked")
        .is_some());
    let running = launcher
        .list()
        .into_iter()
        .find(|a| a.app == "other")
        .unwrap();
    assert_eq!(running.instances[0].state, S::Running);
}
