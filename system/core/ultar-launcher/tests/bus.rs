//! The bus service `launcher` and `ulaunch`: the administrator installs,
//! the shell starts, stops, lists and grants; the shell cannot install even
//! where a policy line lets it call `launcher.install` (no `launcher.admin`).

mod common;

use common::{wait_state, World, ULAUNCH};
use std::process::Command;
use std::sync::{Arc, Mutex};
use ultar_bus::messages::launcher::{InstanceState as S, Role};
use ultar_bus::{Broker, FixedIdentity, Identity, Policy};
use ultar_launcher::service::{self, Admins};
use ultar_service::{Lifecycle, StopCause};

const POLICY: &str = r#"
[[rule]]
identity = "system.launcher"
register = ["launcher"]

[[rule]]
identity = "system.admin"
call = ["launcher.*"]

[[rule]]
identity = "system.shell"
call = ["launcher.start", "launcher.stop", "launcher.list", "launcher.permissions", "launcher.grant", "launcher.revoke", "launcher.install"]
"#;

fn broker(world: &World) -> Broker {
    let fixed = |name: &str| Arc::new(FixedIdentity::new(Identity::new(name).unwrap()));
    Broker::builder(Policy::from_toml_str(POLICY).unwrap())
        .listen(&world.path("bus-launcher.sock"), fixed("system.launcher"))
        .listen(&world.path("admin.sock"), fixed("system.admin"))
        .listen(&world.path("shell.sock"), fixed("system.shell"))
        .start()
        .unwrap()
}

fn ulaunch(world: &World, socket: &str, words: &[&str]) -> (Option<i32>, String, String) {
    let output = Command::new(ULAUNCH)
        .args(["--lang", "en", "--bus"])
        .arg(world.path(socket))
        .args(words)
        .output()
        .unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// `ulaunch list` once the launcher has joined the bus.
fn list(world: &World) -> String {
    let deadline = std::time::Instant::now() + common::WAIT;
    loop {
        let (code, out, err) = ulaunch(world, "admin.sock", &["list"]);
        if code == Some(0) {
            return out;
        }
        assert!(std::time::Instant::now() < deadline, "{err}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn the_shell_steers_apps_and_only_the_administrator_installs() {
    let mut world = World::new("bus");
    let _broker = broker(&world);
    let launcher = world.open_default();
    std::fs::write(world.path("policy.toml"), POLICY).unwrap();
    let admins = Arc::new(Mutex::new(Admins::load(Some(&world.path("policy.toml")))));
    let life = Lifecycle::new();
    let serving = {
        let (launcher, life, socket) = (
            Arc::clone(&launcher),
            life.clone(),
            world.path("bus-launcher.sock"),
        );
        std::thread::spawn(move || service::serve(socket, launcher, admins, life))
    };
    assert!(list(&world).starts_with("app\tuser"));
    let package = world.package("good", "1.0.0", "[permissions]\nnetwork = \"ask\"\n", false);
    let (code, _, err) = ulaunch(&world, "shell.sock", &["install", &package]);
    assert_eq!(code, Some(1), "the shell is no installer");
    assert!(err.contains("not permitted"), "{err}");
    let (code, out, err) = ulaunch(&world, "admin.sock", &["install", &package]);
    assert_eq!(code, Some(0), "{err}");
    assert!(out.contains("good uid=20000 app.standard"), "{out}");
    let (code, out, _) = ulaunch(&world, "shell.sock", &["start", "good", "--token", "9"]);
    assert_eq!(code, Some(0));
    assert!(
        out.contains("running") && out.contains("already=false"),
        "{out}"
    );
    let (_, out, _) = ulaunch(&world, "shell.sock", &["start", "good"]);
    assert!(out.contains("already=true"), "{out}");
    let (_, out, _) = ulaunch(&world, "shell.sock", &["permissions", "good"]);
    assert_eq!(out.trim(), "network\tfalse");
    let (code, out, _) = ulaunch(&world, "shell.sock", &["grant", "good", "network"]);
    assert_eq!(
        (code, out.contains("granted=true")),
        (Some(0), true),
        "{out}"
    );
    let (code, _, err) = ulaunch(&world, "shell.sock", &["grant", "good", "ui.shell"]);
    assert_eq!(code, Some(1));
    assert!(err.contains("does not declare"), "{err}");
    let (code, _, err) = ulaunch(&world, "shell.sock", &["start", "nothing"]);
    assert_eq!(code, Some(1));
    assert!(err.contains("no installed app"), "{err}");
    assert!(list(&world).contains("good\t20000\tapp.standard\t1.0.0\tui\trunning"));
    let (code, out, _) = ulaunch(&world, "shell.sock", &["stop", "good"]);
    assert_eq!((code, out.contains("stopping")), (Some(0), true), "{out}");
    wait_state(&launcher, "good", Role::Ui, &[S::Stopped]);
    let (code, _, _) = ulaunch(&world, "shell.sock", &["uninstall", "good"]);
    assert_eq!(code, Some(1), "the shell does not uninstall either");
    let (code, out, _) = ulaunch(&world, "admin.sock", &["uninstall", "good"]);
    assert_eq!((code, out.contains("stopped=0")), (Some(0), true), "{out}");
    let (code, _, err) = ulaunch(&world, "admin.sock", &["frob"]);
    assert_eq!(code, Some(2), "{err}");
    life.request_stop(StopCause::Request);
    drop(_broker);
    serving.join().unwrap();
}
