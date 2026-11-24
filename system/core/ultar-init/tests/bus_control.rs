//! The bus service `init` and `uinit`: administrators see and steer the
//! services; an identity the broker lets call but that lacks `init.admin`
//! is refused by init itself.

mod common;

use common::{stop, World, UINIT};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::init::{Power, PowerAction};
use ultar_bus::{Broker, FixedIdentity, Identity, Policy};
use ultar_bus::{BusError, Client, ClientOptions, ErrorCode};

const POLICY: &str = r#"
[[rule]]
identity = "system.init"
register = ["init"]

[[rule]]
identity = "system.admin"
call = ["init.status", "init.control", "init.admin"]

[[rule]]
identity = "app.viewer"
call = ["init.status", "init.control", "init.power"]

[[rule]]
identity = "system.powerd"
call = ["init.power", "init.shutdown"]
"#;

fn broker(world: &World) -> Broker {
    std::fs::write(world.path("policy.toml"), POLICY).unwrap();
    let fixed = |name: &str| Arc::new(FixedIdentity::new(Identity::new(name).unwrap()));
    Broker::builder(Policy::from_toml_str(POLICY).unwrap())
        .listen(&world.path("bus-init.sock"), fixed("system.init"))
        .listen(&world.path("admin.sock"), fixed("system.admin"))
        .listen(&world.path("viewer.sock"), fixed("app.viewer"))
        .listen(&world.path("powerd.sock"), fixed("system.powerd"))
        .start()
        .unwrap()
}

fn uinit(world: &World, socket: &str, words: &[&str]) -> (Option<i32>, String, String) {
    let output = Command::new(UINIT)
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

/// `uinit status` once init has joined the bus.
fn status(world: &World) -> String {
    let deadline = std::time::Instant::now() + common::WAIT;
    loop {
        let (code, out, err) = uinit(world, "admin.sock", &["status"]);
        if code == Some(0) {
            return out;
        }
        assert!(std::time::Instant::now() < deadline, "{err}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn an_administrator_sees_and_steers_the_services_others_are_refused() {
    let world = World::new("bus");
    let _broker = broker(&world);
    world.service("calm", "exec = \"/bin/sleep\"\nargs = [\"60\"]");
    let child = world.start("");
    world.wait_status("calm", |c| c[1] == "running");
    let out = status(&world);
    let line = out.lines().find(|line| line.starts_with("calm\t")).unwrap();
    assert!(line.starts_with("calm\trunning\t"), "{out}");
    let (code, out, _) = uinit(&world, "admin.sock", &["stop", "calm"]);
    assert_eq!(code, Some(0));
    assert!(
        out.ends_with("stopping\n") || out.ends_with("stopped\n"),
        "{out}"
    );
    world.wait_status("calm", |c| c[1] == "stopped");
    let (code, _, err) = uinit(&world, "admin.sock", &["stop", "calm"]);
    assert_eq!(
        (code, err.as_str()),
        (Some(1), "The service already is in that state.\n")
    );
    let (code, _, err) = uinit(&world, "admin.sock", &["start", "nonexistent"]);
    assert_eq!(
        (code, err.as_str()),
        (Some(1), "There is no service of that name.\n")
    );
    // The broker lets the viewer call, init refuses it.
    let (code, _, err) = uinit(&world, "viewer.sock", &["status"]);
    assert_eq!(
        (code, err.as_str()),
        (Some(1), "You are not permitted to do this.\n")
    );
    let (code, _, _) = uinit(&world, "viewer.sock", &["start", "calm"]);
    assert_eq!(code, Some(1));
    world.wait_status("calm", |c| c[1] == "stopped");
    let (code, _, err) = uinit(&world, "admin.sock", &["frob"]);
    assert_eq!(code, Some(2), "{err}");
    let (code, stderr) = stop(child);
    assert_eq!(code, Some(0));
    assert!(
        stderr.contains("refused request caller=app.viewer reason=not_admin"),
        "{stderr}"
    );
}

fn power(world: &World, socket: &str, action: PowerAction) -> Result<(), BusError> {
    let client = Client::connect(&world.path(socket), ClientOptions::named("power-test")).unwrap();
    let deadline = std::time::Instant::now() + common::WAIT;
    loop {
        match client.call(&Power { action }, Duration::from_secs(5)) {
            Err(BusError::Remote(ErrorCode::UnknownService))
                if std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            other => return other.map(|_| ()),
        }
    }
}

#[test]
fn only_the_power_service_ends_the_machine_and_init_stops_everything_in_order() {
    let world = World::new("power");
    let _broker = broker(&world);
    world.service("calm", "exec = \"/bin/sleep\"\nargs = [\"60\"]");
    let child = world.start("");
    world.wait_status("calm", |c| c[1] == "running");
    // The broker lets the viewer call init.power; init itself refuses it.
    assert!(matches!(
        power(&world, "viewer.sock", PowerAction::PowerOff),
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
    world.wait_status("calm", |c| c[1] == "running");
    // Not PID 1: init stops every service and ends with code 0.
    assert!(power(&world, "powerd.sock", PowerAction::Reboot).is_ok());
    let (code, stderr) = stop(child);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(
        stderr.contains("refused request caller=app.viewer reason=not_power"),
        "{stderr}"
    );
    assert!(
        stderr.contains("power request caller=system.powerd action=reboot"),
        "{stderr}"
    );
    assert!(stderr.contains("shutdown done"), "{stderr}");
}
