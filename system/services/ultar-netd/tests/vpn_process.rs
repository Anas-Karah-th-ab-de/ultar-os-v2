//! The VPN of the real `ultar-netd` on the bus: without TUN it says so; as
//! root, in a network namespace of its own (`unshare -n`), a `.conf` file is
//! imported (keys 0600, commands refused), the tunnel switched on and off, the
//! wish survives `kill -9`, apps are refused.

use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::Duration;
use ultar_bus::messages::auth::Secret;
use ultar_bus::messages::vpn::{
    ConfProblem, Import, ImportOutcome, RemoveVpn, SetVpn, VpnOutcome, VpnState, VpnStatus,
    VpnUnavailable,
};
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_svckit::testkit::{is_root, wait_until, Daemon, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.netd"
register = ["net", "vpn"]
publish = ["net.state", "net.wifi", "vpn.state"]
subscribe = ["vpn.state"]

[[rule]]
identity = "app.settings"
call = ["vpn.import", "vpn.set", "vpn.remove"]
subscribe = ["vpn.state"]

[[rule]]
identity = "app.any"
call = ["vpn.set"]
"#;

const CONF: &str = "[Interface]\nPrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=\nAddress = 10.8.0.2/32\nDNS = 10.8.0.1\n\
[Peer]\nPublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=\nAllowedIPs = 0.0.0.0/0\nEndpoint = 192.0.2.1:51820\n";

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

fn import(c: &Client, name: &str, text: &str) -> ImportOutcome {
    call(
        c,
        &Import {
            name: name.into(),
            config: Secret::new(text.into()),
        },
    )
    .unwrap()
    .outcome
}

fn set(c: &Client, on: bool) -> VpnOutcome {
    call(c, &SetVpn { on, name: None }).unwrap().outcome
}

/// `ultar-netd` in its own network namespace (root only).
fn spawn_isolated(world: &World, state: &std::path::Path) -> Daemon {
    let token = world.dir.join("system.netd.token");
    std::fs::write(&token, "system.netd").unwrap();
    std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
    let child = Command::new("unshare")
        .args([
            "-n",
            "--",
            env!("CARGO_BIN_EXE_ultar-netd"),
            "--allow-root",
            "--root",
            "/",
            "--state",
        ])
        .arg(state)
        .arg("--run")
        .arg(state.join("run"))
        .arg("--bus")
        .arg(world.dir.join("bus.sock"))
        .arg("--token-file")
        .arg(&token)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    Daemon(child)
}

fn latest(
    sub: &ultar_bus::TypedSubscription<VpnState>,
    ok: impl Fn(&VpnState) -> bool,
) -> VpnState {
    loop {
        let s = sub.recv_timeout(WAIT).unwrap();
        if ok(&s) {
            return s;
        }
    }
}

#[test]
fn without_tun_the_vpn_says_so() {
    let world = World::new(
        "vpn-none",
        POLICY,
        &["system.netd", "app.settings", "app.any"],
    );
    let (root, state) = (world.subdir("root"), world.subdir("state"));
    let mut d = world.spawn(
        env!("CARGO_BIN_EXE_ultar-netd"),
        "system.netd",
        &[
            "--root",
            root.to_str().unwrap(),
            "--state",
            state.to_str().unwrap(),
        ],
    );
    let app = world.client("app.settings");
    let sub = app.subscribe::<VpnState>().unwrap();
    let s = sub.recv_timeout(WAIT).unwrap();
    assert!(!s.available);
    assert_eq!(s.unavailable, Some(VpnUnavailable::NoTun));
    assert_eq!(import(&app, "office", CONF), ImportOutcome::Unavailable);
    assert_eq!(set(&app, true), VpnOutcome::Unavailable);
    let any = world.client("app.any");
    assert!(matches!(
        any.call(
            &RemoveVpn {
                name: "office".into()
            },
            WAIT
        ),
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
    assert_eq!(d.terminate(), Some(0));
}

#[test]
fn import_switch_and_restore_in_a_namespace() {
    let unshare = Command::new("unshare")
        .arg("--version")
        .stdout(Stdio::null())
        .status()
        .is_ok();
    if !is_root() || !unshare || !std::path::Path::new("/dev/net/tun").exists() {
        eprintln!("skipped: needs root, unshare and /dev/net/tun");
        return;
    }
    let world = World::new("vpn", POLICY, &["system.netd", "app.settings", "app.any"]);
    let state = world.subdir("state");
    std::fs::create_dir_all(state.join("run")).unwrap();
    let mut d = spawn_isolated(&world, &state);
    let app = world.client("app.settings");
    let sub = app.subscribe::<VpnState>().unwrap();
    assert!(latest(&sub, |s| s.available).profiles.is_empty());
    imports(&app, &state);
    switch_and_restore(&world, &app, &sub, &state, &mut d);
}

/// Refusals with their line, a saved tunnel 0600 without the key text.
fn imports(app: &Client, state: &std::path::Path) {
    let bad = CONF.replace("DNS = 10.8.0.1", "PostUp = touch /tmp/pwned");
    assert_eq!(
        import(app, "office", &bad),
        ImportOutcome::Invalid {
            problem: ConfProblem::UnknownKey,
            line: 4
        }
    );
    assert_eq!(import(app, "Office", CONF), ImportOutcome::BadName);
    assert_eq!(import(app, "office", CONF), ImportOutcome::Done);
    assert_eq!(import(app, "office", CONF), ImportOutcome::Exists);
    let file = state.join("vpn/tunnels/office");
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!String::from_utf8_lossy(&std::fs::read(&file).unwrap()).contains("yAnz5TF"));
}

/// On, DNS first in resolv.conf, kill -9 restores it, off, remove.
fn switch_and_restore(
    world: &World,
    app: &Client,
    sub: &ultar_bus::TypedSubscription<VpnState>,
    state: &std::path::Path,
    d: &mut Daemon,
) {
    let file = state.join("vpn/tunnels/office");
    assert_eq!(set(app, true), VpnOutcome::Done);
    let s = latest(sub, |s| s.active.is_some());
    assert_eq!(s.active.as_deref(), Some("office"));
    assert_eq!(s.status, VpnStatus::Connecting, "no server answers here");
    assert_eq!(s.dns, vec!["10.8.0.1".to_string()]);
    assert!(s.profiles[0].full_tunnel);
    // The network service puts the tunnel's DNS server first.
    let resolv = state.join("run/resolv.conf");
    assert!(wait_until(WAIT, || std::fs::read_to_string(&resolv)
        .is_ok_and(|t| t.contains("nameserver 10.8.0.1"))));
    // kill -9: the wish comes back with the next start.
    d.crash();
    let mut again = spawn_isolated(world, state);
    let s = latest(sub, |s| s.active.is_some());
    assert_eq!(s.active.as_deref(), Some("office"));
    assert_eq!(set(app, false), VpnOutcome::Done);
    assert_eq!(latest(sub, |s| s.active.is_none()).status, VpnStatus::Off);
    assert!(wait_until(WAIT, || std::fs::read_to_string(&resolv)
        .is_ok_and(|t| !t.contains("10.8.0.1"))));
    assert_eq!(
        call(
            app,
            &RemoveVpn {
                name: "office".into()
            }
        )
        .unwrap()
        .outcome,
        VpnOutcome::Done
    );
    assert!(!file.exists());
    assert_eq!(set(app, true), VpnOutcome::NotFound);
    assert_eq!(again.terminate(), Some(0));
}
