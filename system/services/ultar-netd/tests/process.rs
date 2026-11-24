//! The real `ultar-netd` on a prepared kernel tree: state, Wi-Fi and airplane switches
//! kept across a restart, honest "no device" for Wi-Fi networks on a kernel without
//! Wi-Fi (this host has no cfg80211), apps refused.

use std::time::Duration;
use ultar_bus::messages::net::{
    Connect, Join, JoinOutcome, Known, NetOutcome, NetState, Networks, Scan, SetAirplane, SetWifi,
    WifiLink, WifiPhase,
};
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_svckit::testkit::{wait_until, Daemon, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.netd"
register = ["net", "vpn"]
publish = ["net.state", "net.wifi", "vpn.state"]
subscribe = ["vpn.state"]

[[rule]]
identity = "system.shell"
call = ["net.set_wifi", "net.set_airplane", "net.scan", "net.connect", "net.networks", "net.join", "net.known"]
subscribe = ["net.state", "net.wifi"]

[[rule]]
identity = "app.any"
call = []
"#;

fn put(root: &std::path::Path, path: &str, value: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, value).unwrap();
}

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(10));
        // Right after kill -9 the broker may still route to the dead
        // connection (ServiceGone) until it sees it closed: ask again.
        !matches!(
            last,
            Err(BusError::Remote(
                ErrorCode::UnknownService | ErrorCode::ServiceGone
            ))
        )
    });
    last
}

fn start(world: &World) -> Daemon {
    let (root, state) = (world.dir.join("root"), world.subdir("state"));
    world.spawn(
        env!("CARGO_BIN_EXE_ultar-netd"),
        "system.netd",
        &[
            "--root",
            root.to_str().unwrap(),
            "--state",
            state.to_str().unwrap(),
        ],
    )
}

/// This host's kernel has no cfg80211: every Wi-Fi call says "no device".
fn no_wifi_device(shell: &Client) {
    let wifi = shell.subscribe::<WifiLink>().unwrap();
    assert_eq!(wifi.recv_timeout(WAIT).unwrap().phase, WifiPhase::NoDevice);
    let scan = call(shell, &Scan {}).unwrap();
    assert_eq!(
        (scan.outcome, scan.networks.len()),
        (NetOutcome::NoDevice, 0)
    );
    let all = call(shell, &Networks {}).unwrap();
    assert_eq!(all.outcome, NetOutcome::NoDevice);
    let join = Join {
        ssid: "home".into(),
        secret: None,
        remember: true,
    };
    assert_eq!(call(shell, &join).unwrap().outcome, JoinOutcome::NoDevice);
    let connect = Connect {
        ssid: "home".into(),
        secret: None,
    };
    assert_eq!(call(shell, &connect).unwrap().outcome, NetOutcome::NoDevice);
    assert!(call(shell, &Known {}).unwrap().ssids.is_empty());
}

#[test]
fn radios_switch_and_the_wish_survives_a_restart() {
    let world = World::new("net", POLICY, &["system.netd", "system.shell", "app.any"]);
    let root = world.subdir("root");
    put(&root, "sys/class/net/wlan0/type", "1");
    std::fs::create_dir_all(root.join("sys/class/net/wlan0/wireless")).unwrap();
    put(&root, "sys/class/rfkill/rfkill0/type", "wlan");
    put(&root, "sys/class/rfkill/rfkill0/soft", "0");
    put(&root, "dev/rfkill", "");
    let mut daemon = start(&world);
    let shell = world.client("system.shell");
    let state = shell.subscribe::<NetState>().unwrap();
    let s = state.recv_timeout(WAIT).unwrap();
    assert!(s.wifi_present && s.wifi_on && !s.networks_supported);
    no_wifi_device(&shell);
    assert_eq!(
        call(&shell, &SetAirplane { on: true }).unwrap().outcome,
        NetOutcome::Done
    );
    assert_eq!(
        std::fs::read(root.join("dev/rfkill")).unwrap(),
        [0, 0, 0, 0, 1, 3, 1, 0]
    );
    let app = world.client("app.any");
    assert!(matches!(
        app.call(&SetWifi { on: true }, WAIT),
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
    // A new start blocks the radio again (the kernel would have it on).
    daemon.crash();
    put(&root, "dev/rfkill", "");
    let mut again = start(&world);
    assert_eq!(
        call(&shell, &Scan {}).unwrap().outcome,
        NetOutcome::NoDevice
    );
    let ok = wait_until(WAIT, || {
        std::fs::read(root.join("dev/rfkill")).unwrap() == [0, 0, 0, 0, 1, 3, 1, 0]
    });
    assert!(ok, "{:?}", std::fs::read(root.join("dev/rfkill")).unwrap());
    assert_eq!(again.terminate(), Some(0));
}
