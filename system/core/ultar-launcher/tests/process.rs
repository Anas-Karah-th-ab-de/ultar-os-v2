//! Process tests of the launcher: real helper and app processes, as root
//! (real cells) and without root (unconfined by the operator's explicit
//! `accept_missing`; the CI runs them as a user with umask 002).

mod common;

use common::{root, wait_state, Collector, World};
use ultar_bus::messages::launcher::{Exit, InstanceState as S, Role};
use ultar_launcher::error::{LaunchError, PackageError};
use ultar_launcher::manifest::ManifestError;
use ultar_launcher::perm::Permission;
use ultar_launcher::platform::Feature;
use ultar_launcher::profile::ProfileError;

#[test]
fn start_is_single_instance_and_stop_ends_the_cell() {
    let mut world = World::new("single");
    let launcher = world.open_default();
    let reply = launcher
        .install(&world.package("good", "1.0.0", "", true))
        .unwrap();
    assert_eq!((reply.uid, reply.profile.as_str()), (20000, "app.system"));
    let first = launcher.start("good", Role::Ui, None).unwrap();
    assert_eq!((first.state, first.already), (S::Running, false));
    assert!(first.pid > 0);
    let again = launcher.start("good", Role::Ui, None).unwrap();
    assert_eq!(
        (again.state, again.pid, again.already),
        (S::Running, first.pid, true)
    );
    let listed = launcher.list();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].instances[0].state, S::Running);
    assert_eq!(launcher.stop("good", Role::Ui).unwrap().state, S::Stopping);
    let ended = wait_state(&launcher, "good", Role::Ui, &[S::Stopped]);
    // the stop signal reached the app (pid 1 of the cell passes it on)
    assert_eq!((ended.last_exit, ended.pid), (Some(Exit::Signal(15)), 0));
    assert_eq!(
        launcher.stop("missing", Role::Ui),
        Err(LaunchError::UnknownApp)
    );
    assert_eq!(
        launcher.start("Bad Id", Role::Ui, None),
        Err(LaunchError::Invalid)
    );
}

#[test]
fn output_goes_to_the_log_service_under_the_apps_name() {
    let mut world = World::new("output");
    let collector = Collector::listen(&world.path("out.sock"));
    let launcher = world.open_default();
    launcher
        .install(&world.package_args("good", &["info"], "", true))
        .unwrap();
    launcher.start("good", Role::Ui, Some(77)).unwrap();
    let line = collector
        .wait_for("app-good", "TESTAPP uid=")
        .expect("no output line");
    assert!(collector
        .wait_for("app-good", "TESTAPP stderr line")
        .is_some());
    let ended = wait_state(&launcher, "good", Role::Ui, &[S::Stopped, S::Crashed]);
    assert_eq!(ended.last_exit, Some(Exit::Code(0)), "{line}");
    assert!(
        line.contains("token=77") && line.contains("app=good"),
        "{line}"
    );
    assert!(
        line.contains("data_writable=true") && line.contains("tmp_writable=true"),
        "{line}"
    );
    if root() {
        // its own user, its own data directory as working directory, no
        // kernel rights (Linux: the Ultar prctl is unknown), a read-only root
        assert!(line.contains("uid=20000 gid=20000"), "{line}");
        assert!(
            line.contains("cwd=/data") && line.contains("root_readonly=true"),
            "{line}"
        );
        assert!(line.contains("rights=none"), "{line}");
    } else {
        assert!(
            line.contains(&format!("uid={}", ultar_sys::uid())),
            "{line}"
        );
    }
}

#[test]
fn crashes_and_exit_codes_of_windows_are_reported_not_restarted() {
    let mut world = World::new("crash");
    let launcher = world.open_default();
    launcher
        .install(&world.package_args("crashy", &["crash"], "", true))
        .unwrap();
    launcher.start("crashy", Role::Ui, None).unwrap();
    let crashed = wait_state(&launcher, "crashy", Role::Ui, &[S::Crashed]);
    assert_eq!(crashed.last_exit, Some(Exit::Signal(6)));
    assert_eq!(crashed.restarts, 0, "a window is never restarted");
    launcher
        .install(&world.package_args("other", &["exit", "7"], "", true))
        .unwrap();
    launcher.start("other", Role::Ui, None).unwrap();
    assert_eq!(
        wait_state(&launcher, "other", Role::Ui, &[S::Crashed]).last_exit,
        Some(Exit::Code(7))
    );
}

#[test]
fn services_are_restarted_and_need_the_background_permission() {
    let mut world = World::new("service");
    let launcher = world.open_default();
    launcher
        .install(&world.package_args("crashy", &["crash"], "", true))
        .unwrap();
    // a background service that fails: restarted with back-off, then given up
    let service = format!(
        "[permissions]\nbackground = true\n\n[service]\nexec = \"{}\"\nargs = [\"exit\", \"3\"]\nrestart = \"on-failure\"\n",
        common::TESTAPP
    );
    launcher
        .install(&world.package("svc", "1.0.0", &service, true))
        .unwrap();
    launcher.start("svc", Role::Service, None).unwrap();
    let failed = wait_state(&launcher, "svc", Role::Service, &[S::Failed]);
    assert_eq!(failed.last_exit, Some(Exit::Code(3)));
    assert_eq!(
        failed.restarts as usize,
        ultar_launcher::policy::LOOP_FAILURES - 1
    );
    // a store app must be granted `background` first
    launcher
        .install(&world.package("store.good", "1.0.0", &service, false))
        .unwrap();
    assert_eq!(
        launcher.start("store.good", Role::Service, None),
        Err(LaunchError::NotGranted(Permission::Background))
    );
    assert_eq!(
        launcher.start("crashy", Role::Service, None),
        Err(LaunchError::NotDeclared)
    );
    assert!(
        launcher
            .set_permission("store.good", "background", true)
            .unwrap()
            .granted
    );
    launcher.start("store.good", Role::Service, None).unwrap();
    launcher.stop("store.good", Role::Service).unwrap();
    assert!(matches!(
        wait_state(
            &launcher,
            "store.good",
            Role::Service,
            &[S::Stopped, S::Backoff, S::Failed, S::Crashed]
        )
        .state,
        S::Stopped | S::Backoff | S::Failed | S::Crashed
    ));
}

#[test]
fn user_ids_are_stable_across_reinstall_and_restart_and_never_reused() {
    let mut world = World::new("ids");
    let config = if root() {
        let tree = world.fake_group_tree();
        world.config_text(&[], Some(&tree))
    } else {
        world.config_text(&["ids", "cell", "groups", "net"], None)
    };
    let launcher = world.open(&config);
    let a = launcher
        .install(&world.package("good", "1.0.0", "", true))
        .unwrap()
        .uid;
    let b = launcher
        .install(&world.package("other", "1.0.0", "", true))
        .unwrap()
        .uid;
    assert_eq!((a, b), (20000, 20001));
    std::fs::write(world.path("appdata/good/note"), b"keep?").unwrap();
    assert_eq!(launcher.uninstall("good", false), Ok(0));
    assert!(
        !world.path("appdata/good").exists(),
        "data removed with the app"
    );
    assert_eq!(
        launcher.start("good", Role::Ui, None),
        Err(LaunchError::UnknownApp)
    );
    let c = launcher
        .install(&world.package("evil", "1.0.0", "", true))
        .unwrap()
        .uid;
    assert_eq!(c, 20002, "a removed app's id is not given to another app");
    let again = launcher
        .install(&world.package("good", "2.0.0", "", true))
        .unwrap()
        .uid;
    assert_eq!(again, a, "a reinstall gets its own id back");
    assert!(!world.path("appdata/good/note").exists());
    std::fs::write(world.path("appdata/other/note"), b"kept").unwrap();
    launcher.uninstall("other", true).unwrap();
    assert!(
        world.path("appdata/other/note").exists(),
        "keep_data keeps the data"
    );
    after_a_restart(&mut world, &config, b);
}

/// The state survives a restart of the launcher; `other` (uninstalled,
/// data kept) gets `uid` back.
fn after_a_restart(world: &mut World, config: &str, uid: u32) {
    world.close();
    let launcher = world.open(config);
    let listed: Vec<(String, u32, String)> = launcher
        .list()
        .into_iter()
        .map(|a| (a.app, a.uid, a.version))
        .collect();
    assert_eq!(
        listed,
        [
            ("evil".into(), 20002, "1.0.0".into()),
            ("good".into(), 20000, "2.0.0".into())
        ]
    );
    let again = launcher.install(&world.package("other", "1.1.0", "", true));
    assert_eq!(again.unwrap().uid, uid);
    if root() {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(world.path("appdata/good")).unwrap();
        assert_eq!(
            (meta.uid(), meta.gid(), meta.mode() & 0o7777),
            (20000, 20000, 0o700)
        );
    }
}

#[test]
fn packages_beyond_their_profile_are_refused() {
    let mut world = World::new("packages");
    let launcher = world.open_default();
    let refused = |result: Result<_, LaunchError>| result.map(|_| ()).unwrap_err();
    // a store app may not ask for compositor rights or more than its profile
    let ui = world.package(
        "store.good",
        "1.0.0",
        "[permissions]\nui = [\"ui.capture\"]\n",
        false,
    );
    assert_eq!(
        refused(launcher.install(&ui)),
        LaunchError::Package(PackageError::Profile(ProfileError::Permission(
            Permission::UiCapture
        )))
    );
    let big = world.package("store.good", "1.0.1", "[limits]\nmemory_mb = 4096\n", false);
    assert_eq!(
        refused(launcher.install(&big)),
        LaunchError::Package(PackageError::Profile(ProfileError::Limit))
    );
    let kernel = world.package(
        "good",
        "1.0.2",
        "[permissions]\nkernel = [\"mount\"]\n",
        true,
    );
    assert_eq!(
        refused(launcher.install(&kernel)),
        LaunchError::Package(PackageError::Profile(ProfileError::KernelRights))
    );
    assert!(launcher.list().is_empty(), "nothing was registered");
}

#[test]
fn unsigned_foreign_and_broken_packages_are_refused() {
    let mut world = World::new("unsigned");
    let launcher = world.open_default();
    let refused = |result: Result<_, LaunchError>| result.map(|_| ()).unwrap_err();
    world.package("good", "1.0.0", "", false);
    // a package signed by nobody the device trusts
    let (stranger, _) = ultar_pkg::keygen();
    let forged = world.path("forged.uap");
    ultar_pkg::pack_and_sign(
        &world.path("manifest-good-1.0.0.toml"),
        &world.path("src"),
        &forged,
        &stranger,
    )
    .unwrap();
    assert_eq!(
        refused(launcher.install(forged.to_str().unwrap())),
        LaunchError::Package(PackageError::Signature)
    );
    let compat = world.sign(
        "compat",
        "id = \"compat\"\nversion = \"1\"\nkind = \"compat-deb\"\nname = \"x\"\n",
        true,
    );
    assert_eq!(
        refused(launcher.install(&compat)),
        LaunchError::Package(PackageError::Manifest(ManifestError::NotNative))
    );
    let legacy = world.sign("legacy", "id = \"legacy\"\nversion = \"1\"\nkind = \"native\"\nname = \"x\"\nexec = \"/bin/x\"\npermissions = [\"network\"]\n", true);
    assert_eq!(
        refused(launcher.install(&legacy)),
        LaunchError::Package(PackageError::Manifest(ManifestError::Syntax))
    );
    assert_eq!(
        refused(launcher.install("relative.uap")),
        LaunchError::Invalid
    );
    assert_eq!(
        refused(launcher.install("/nonexistent/x.uap")),
        LaunchError::Package(PackageError::Read)
    );
    assert!(launcher.list().is_empty(), "nothing was registered");
}

#[test]
fn starts_fail_closed_without_the_isolation_the_operator_requires() {
    let mut world = World::new("closed");
    let config = world.config_text(&[], None);
    let launcher = world.open(&config);
    let package = world.package("good", "1.0.0", "", true);
    if !root() {
        // no own user ids: not even installed (its data would not be its own)
        assert_eq!(
            launcher.install(&package).map(|_| ()),
            Err(LaunchError::Platform(Feature::Ids))
        );
        assert!(launcher.list().is_empty());
        return;
    }
    launcher.install(&package).unwrap();
    // as root: no group tree configured – no start without limits
    assert_eq!(
        launcher.start("good", Role::Ui, None).map(|_| ()),
        Err(LaunchError::Platform(Feature::Groups))
    );
    assert!(
        launcher.list()[0].instances.is_empty(),
        "nothing was started"
    );
    {
        // a tree without the limit files (a kernel without the controllers)
        let tree = world.path("bare-groups");
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(tree.join("cgroup.procs"), b"").unwrap();
        world.close();
        let config = world.config_text(&[], Some(&tree));
        let launcher = world.open(&config);
        assert_eq!(
            launcher.start("good", Role::Ui, None),
            Err(LaunchError::Platform(Feature::Groups))
        );
    }
}
