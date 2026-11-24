//! The real `ultar-btd` on a machine tree without Bluetooth: it says so; devices are
//! honestly unsupported; apps are refused.

use std::time::Duration;
use ultar_bus::messages::bluetooth::{BluetoothState, BtOutcome, Devices, Set};
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_svckit::testkit::{wait_until, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.btd"
register = ["bluetooth"]
publish = ["bluetooth.state"]

[[rule]]
identity = "system.shell"
call = ["bluetooth.set", "bluetooth.devices", "bluetooth.connect", "bluetooth.disconnect"]
subscribe = ["bluetooth.state"]

[[rule]]
identity = "app.any"
call = []
"#;

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(10));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

#[test]
fn without_bluetooth_the_service_says_so() {
    let world = World::new("bt", POLICY, &["system.btd", "system.shell", "app.any"]);
    let (root, state) = (world.subdir("root"), world.subdir("state"));
    let mut daemon = world.spawn(
        env!("CARGO_BIN_EXE_ultar-btd"),
        "system.btd",
        &[
            "--root",
            root.to_str().unwrap(),
            "--state",
            state.to_str().unwrap(),
        ],
    );
    let shell = world.client("system.shell");
    let s = shell
        .subscribe::<BluetoothState>()
        .unwrap()
        .recv_timeout(WAIT)
        .unwrap();
    assert!(!s.present && !s.devices_supported);
    assert_eq!(
        call(&shell, &Set { on: true }).unwrap().outcome,
        BtOutcome::NoDevice
    );
    assert_eq!(
        call(&shell, &Devices {}).unwrap().outcome,
        BtOutcome::Unsupported
    );
    let app = world.client("app.any");
    assert!(matches!(
        app.call(&Set { on: false }, WAIT),
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
    assert_eq!(daemon.terminate(), Some(0));
}
