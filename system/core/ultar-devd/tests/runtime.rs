//! The device daemon on the service runtime: stop signals, recovery after a kill,
//! limits, the `devices.admin` permission, the retained capability set after a broker
//! restart, and logging of refused operations.

mod rig;

use rig::kernel_fmt::beelink;
use rig::{builtin_manifest, Rig};
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::device_service::{
    ClearQuarantine, List, Outcome, Rescan, Rollback, Shutdown, Switch,
};
use ultar_bus::messages::devices::Capabilities;
use ultar_bus::{
    Broker, BusError, Client, ClientOptions, ErrorCode, FixedIdentity, Identity, PeerIdentity,
    Policy, TokenIdentity,
};
use ultar_devd::ChoiceStore;

const WAIT: Duration = Duration::from_secs(30);
const NVME0: &str = "pci:01:00.0";
const DAEMON: &str = env!("CARGO_BIN_EXE_ultar-devd");

/// The policy of a real system: the settings panel is the administrator,
/// a viewer app may look and ask for a new search – nothing else, even
/// though its rule names the calls.
const POLICY: &str = r#"
[[rule]]
identity  = "system.devd"
register  = ["devices"]
publish   = ["devices.*"]

[[rule]]
identity  = "app.settings"
call      = ["devices.*"]
subscribe = ["devices.*"]

[[rule]]
identity  = "app.viewer"
call      = ["devices.list", "devices.rescan", "devices.switch", "devices.rollback",
             "devices.clear_quarantine", "devices.shutdown"]
subscribe = ["devices.*"]
"#;

const TOKENS: [(&str, &str); 3] = [
    ("system.devd", "devd-token-0123456789"),
    ("app.settings", "panel-token-0123456789"),
    ("app.viewer", "viewer-token-0123456789"),
];

fn send_signal(pid: u32, signal: &str) {
    let status = Command::new("kill")
        .args([&format!("-{signal}"), &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success());
}

fn switch_rig(tag: &str) -> Rig {
    let rig = Rig::new(tag);
    let machine = beelink();
    machine.write_ultar_root(&rig.root, &machine.selftest_log());
    rig.add_repo_drivers();
    rig.add_planned_drivers();
    let rule = "pci_class = [\"010802\"]";
    rig.add_builtin(&builtin_manifest("nvme", "1.1.0", "storage", rule), "nvme");
    rig.write_index();
    rig
}

fn dir_of(rig: &Rig) -> PathBuf {
    rig.root.parent().unwrap().to_owned()
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn open_broker(rig: &Rig) -> Broker {
    let identity: Arc<dyn PeerIdentity> =
        Arc::new(FixedIdentity::new(Identity::new("test.peer").unwrap()));
    Broker::builder(Policy::allow_all())
        .listen(&dir_of(rig).join("bus.sock"), identity)
        .start()
        .unwrap()
}

fn daemon(rig: &Rig, extra: &[&str]) -> Command {
    let mut command = Command::new(DAEMON);
    command
        .args(["--depot", &text(&rig.depot), "--keys", &text(&rig.keys)])
        .args(["--state", &text(&rig.state), "--root", &text(&rig.root)])
        .args([
            "--bus",
            &text(&dir_of(rig).join("bus.sock")),
            "--lang",
            "en",
        ])
        .args(extra)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if ultar_service::effective_uid() == 0 {
        command.arg("--allow-root");
    }
    command
}

fn connect(rig: &Rig, token: Option<&str>) -> Client {
    let options = ClientOptions {
        token: token.map(str::to_owned),
        ..ClientOptions::named("test")
    };
    Client::connect(&dir_of(rig).join("bus.sock"), options).unwrap()
}

/// Waits until the daemon serves (its retained capability set is there).
fn wait_started(client: &Client) -> Capabilities {
    let capabilities = client.subscribe::<Capabilities>().unwrap();
    capabilities.recv_timeout(WAIT).unwrap()
}

fn finish(child: Child) -> (Option<i32>, String) {
    let output = child.wait_with_output().unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn sigterm_and_sigint_stop_cleanly_with_exit_code_zero() {
    for signal in ["TERM", "INT"] {
        let rig = switch_rig(&format!("rt-{signal}"));
        let _broker = open_broker(&rig);
        let child = daemon(&rig, &["--allow-all"]).spawn().unwrap();
        let client = connect(&rig, None);
        wait_started(&client);
        send_signal(child.id(), signal);
        let (code, stderr) = finish(child);
        assert_eq!(code, Some(0), "{signal}: {stderr}");
        assert!(
            stderr.ends_with("ultar-devd: The device service was stopped.\n"),
            "{stderr}"
        );
        // The service name is free again: nothing answers. The broker
        // learns of the closed connection on its own thread, so a call right
        // after the exit may still reach the vanishing service.
        let deadline = std::time::Instant::now() + WAIT;
        while !matches!(
            client.call(&List {}, WAIT),
            Err(BusError::Remote(ErrorCode::UnknownService))
        ) {
            assert!(
                std::time::Instant::now() < deadline,
                "{signal}: still served"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

#[test]
fn a_choice_survives_sigkill_and_the_daemon_serves_again() {
    let rig = switch_rig("rt-kill");
    let _broker = open_broker(&rig);
    let mut child = daemon(&rig, &["--allow-all"]).spawn().unwrap();
    let client = connect(&rig, None);
    wait_started(&client);
    let switch = Switch {
        address: NVME0.into(),
        driver: "nvme".into(),
        version: "1.0.0".into(),
    };
    let reply = client.call(&switch, WAIT).unwrap();
    assert!(
        matches!(reply.outcome, Outcome::Done(_)),
        "{:?}",
        reply.outcome
    );
    // Killed right after the reply: the choice was on disk before it.
    send_signal(child.id(), "KILL");
    assert_eq!(child.wait().unwrap().code(), None);
    let mut store = ultar_devd::store::file::FileStore::new(&rig.state);
    let state = store.load().unwrap();
    let choice = state.choices.get("pci:01:00.0=pci:c0a9:560b").unwrap();
    assert_eq!(choice.to_string(), "nvme 1.0.0");
    // Restart: the daemon comes up with the remembered choice.
    let child = daemon(&rig, &["--allow-all"]).spawn().unwrap();
    let list = loop {
        match client.call(&List {}, WAIT) {
            Ok(list) => break list,
            // Not registered yet – or the broker is only now noticing that
            // the killed instance's connection is dead.
            Err(BusError::Remote(ErrorCode::UnknownService | ErrorCode::ServiceGone)) => {
                std::thread::yield_now();
            }
            Err(other) => panic!("{other}"),
        }
    };
    let nvme = list.devices.iter().find(|d| d.address == NVME0).unwrap();
    assert_eq!(
        nvme.active.as_ref().map(|d| d.version.as_str()),
        Some("1.0.0")
    );
    send_signal(child.id(), "TERM");
    assert_eq!(finish(child).0, Some(0));
}

#[test]
fn limits_are_applied_and_root_and_access_must_be_declared() {
    let rig = switch_rig("rt-limits");
    let _broker = open_broker(&rig);
    let child = daemon(&rig, &["--allow-all"]).spawn().unwrap();
    wait_started(&connect(&rig, None));
    let limits = std::fs::read_to_string(format!("/proc/{}/limits", child.id())).unwrap();
    let has = |name: &str, value: &str| {
        limits.lines().any(|line| {
            line.starts_with(name) && line[name.len()..].split_whitespace().next() == Some(value)
        })
    };
    assert!(has("Max open files", "256"), "{limits}");
    // No address-space limit of its own (review round 3, R3-B0): the
    // group's memory.max bounds memory; the value is the inherited one.
    assert!(has(
        "Max address space",
        &own_soft_limit("Max address space")
    ));
    assert!(has("Max core file size", "0"), "{limits}");
    send_signal(child.id(), "TERM");
    assert_eq!(finish(child).0, Some(0));

    // Neither --policy nor --allow-all: refused with its own message.
    let (code, stderr) = finish(daemon(&rig, &[]).spawn().unwrap());
    assert_eq!(code, Some(2));
    assert!(
        stderr
            .starts_with("A policy file (--policy) or --allow-all is required.\nUsage: ultar-devd"),
        "{stderr}"
    );
    let (code, stderr) = finish(
        daemon(&rig, &["--allow-all", "--policy", "/p"])
            .spawn()
            .unwrap(),
    );
    assert_eq!(code, Some(2));
    assert!(
        stderr.starts_with("--policy and --allow-all cannot be combined."),
        "{stderr}"
    );
    let (code, stderr) = finish(
        daemon(&rig, &["--policy", "/no/such/file"])
            .spawn()
            .unwrap(),
    );
    assert_eq!(code, Some(1));
    assert!(
        stderr.starts_with("ultar-devd: The policy file is not valid:"),
        "{stderr}"
    );
    root_must_be_declared(&rig);
}

/// As root without `--allow-root` the daemon refuses to start.
fn root_must_be_declared(rig: &Rig) {
    if ultar_service::effective_uid() != 0 {
        return;
    }
    let command = daemon(rig, &["--allow-all"]);
    let args: Vec<_> = command
        .get_args()
        .filter(|arg| *arg != "--allow-root")
        .map(ToOwned::to_owned)
        .collect();
    let output = Command::new(DAEMON).args(args).output().unwrap();
    assert_eq!(output.status.code(), Some(71));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "ultar-devd: The device service does not run as root without --allow-root.\n"
    );
}

/// A broker with token identities and the policy above, the daemon
/// started with its token from a file and the same policy.
struct Guarded {
    rig: Rig,
    _broker: Broker,
    child: Child,
    log: UnixListener,
}

fn guarded(tag: &str) -> Guarded {
    let rig = switch_rig(tag);
    let dir = dir_of(&rig);
    let mut auth = TokenIdentity::new();
    for (identity, token) in TOKENS {
        auth = auth.with_token(token, Identity::new(identity).unwrap());
    }
    let broker = Broker::builder(Policy::from_toml_str(POLICY).unwrap())
        .listen(&dir.join("bus.sock"), Arc::new(auth))
        .start()
        .unwrap();
    std::fs::write(dir.join("policy.toml"), POLICY).unwrap();
    std::fs::write(dir.join("token"), TOKENS[0].1).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir.join("token"), std::fs::Permissions::from_mode(0o600)).unwrap();
    let log = UnixListener::bind(dir.join("log.sock")).unwrap();
    let child = daemon(
        &rig,
        &[
            "--policy",
            &text(&dir.join("policy.toml")),
            "--token-file",
            &text(&dir.join("token")),
            "--log-socket",
            &text(&dir.join("log.sock")),
        ],
    )
    .spawn()
    .unwrap();
    Guarded {
        rig,
        _broker: broker,
        child,
        log,
    }
}

fn switch_nvme() -> Switch {
    Switch {
        address: NVME0.into(),
        driver: "nvme".into(),
        version: "1.0.0".into(),
    }
}

/// Everything the daemon wrote to its log socket, as (key, fields) pairs.
fn logged(world: Guarded) -> Vec<(String, String)> {
    assert_eq!(finish(world.child).0, Some(0));
    let (mut stream, _) = world.log.accept().unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let mut records = Vec::new();
    ultar_service::log::wire::FrameReader::new()
        .push(&bytes, |body| {
            let record = ultar_service::log::wire::decode_body(body).unwrap();
            records.push((record.key, format!("{:?}", record.fields)));
        })
        .unwrap();
    records
}

#[test]
fn without_devices_admin_nothing_can_be_changed_or_stopped() {
    let world = guarded("rt-viewer");
    let viewer = connect(&world.rig, Some(TOKENS[2].1));
    wait_started(&viewer);
    // Reading and asking for a new search are open to the viewer.
    assert!(!viewer.call(&List {}, WAIT).unwrap().devices.is_empty());
    let rescan = viewer.call(&Rescan { address: None }, WAIT).unwrap();
    assert!(rescan.devices.is_ok());
    let mut store = ultar_devd::store::file::FileStore::new(&world.rig.state);
    let before = format!("{:?}", store.load().unwrap());
    // The four administrative calls are refused although the broker's
    // policy lets the calls through (review finding 18).
    let denied = |result: Result<(), BusError>| {
        let refused = matches!(result, Err(BusError::Remote(ErrorCode::PermissionDenied)));
        assert!(refused, "{result:?}");
    };
    denied(viewer.call(&switch_nvme(), WAIT).map(|_| ()));
    let rollback = Rollback {
        address: NVME0.into(),
    };
    denied(viewer.call(&rollback, WAIT).map(|_| ()));
    denied(viewer.call(&ClearQuarantine::default(), WAIT).map(|_| ()));
    denied(viewer.call(&Shutdown {}, WAIT).map(|_| ()));
    // Nothing changed, and the daemon is still there.
    assert_eq!(format!("{:?}", store.load().unwrap()), before);
    assert!(!viewer.call(&List {}, WAIT).unwrap().devices.is_empty());
    send_signal(world.child.id(), "TERM");
    // Every refusal was logged with operation and peer – through the log
    // socket, not on standard error.
    let records = logged(world);
    let refused: Vec<&String> = records
        .iter()
        .filter(|(key, _)| key == "devd.admin_refused")
        .map(|(_, fields)| fields)
        .collect();
    assert_eq!(refused.len(), 4, "{records:?}");
    let operations = ["switch", "rollback", "clear_quarantine", "shutdown"];
    for (line, operation) in refused.iter().zip(operations) {
        assert!(line.contains(&format!("Word(\"{operation}\")")), "{line}");
        assert!(line.contains("Id(\"app.viewer\")"), "{line}");
    }
}

#[test]
fn an_identity_with_devices_admin_may_switch_roll_back_clear_and_stop() {
    let world = guarded("rt-admin");
    let panel = connect(&world.rig, Some(TOKENS[1].1));
    wait_started(&panel);
    let reply = panel.call(&switch_nvme(), WAIT).unwrap();
    assert!(
        matches!(reply.outcome, Outcome::Done(_)),
        "{:?}",
        reply.outcome
    );
    let rollback = Rollback {
        address: NVME0.into(),
    };
    panel.call(&rollback, WAIT).unwrap();
    let cleared = panel.call(&ClearQuarantine::default(), WAIT).unwrap();
    assert!(cleared.released.is_ok());
    panel.call(&Shutdown {}, WAIT).unwrap();
    let records = logged(world);
    let keys: Vec<&str> = records.iter().map(|(key, _)| key.as_str()).collect();
    assert!(!keys.contains(&"devd.admin_refused"), "{keys:?}");
    for key in [
        "service.starting",
        "devd.started",
        "service.ready",
        "devd.stopped",
        "service.stopped",
    ] {
        assert!(keys.contains(&key), "{key} missing in {keys:?}");
    }
}

#[test]
fn the_capability_set_is_published_again_after_a_broker_restart() {
    let rig = switch_rig("rt-retained");
    let first = open_broker(&rig);
    let child = daemon(&rig, &["--allow-all"]).spawn().unwrap();
    let before = wait_started(&connect(&rig, None));
    // The broker goes away and comes back with empty memory.
    drop(first);
    let _second = open_broker(&rig);
    let late = connect(&rig, None);
    let after = wait_started(&late);
    assert_eq!(after, before);
    // The service is registered again as well.
    let list = loop {
        match late.call(&List {}, WAIT) {
            Ok(list) => break list,
            Err(BusError::Remote(ErrorCode::UnknownService)) => std::thread::yield_now(),
            Err(other) => panic!("{other}"),
        }
    };
    assert!(!list.devices.is_empty());
    send_signal(child.id(), "TERM");
    assert_eq!(finish(child).0, Some(0));
}

#[test]
fn the_shipped_service_definition_is_a_command_line_the_daemon_accepts() {
    let file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../rootfs/etc/ultar/services/devd.toml");
    let table: toml::Table = std::fs::read_to_string(file).unwrap().parse().unwrap();
    let args: Vec<String> = table["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap().to_owned())
        .collect();
    let args = ultar_devd::runner::parse(&args).unwrap();
    assert_eq!(
        args.bus.as_deref(),
        Some(Path::new("/run/ultar/bus/devd.sock"))
    );
    assert_eq!(
        args.policy.as_deref(),
        Some(Path::new("/etc/ultar/bus-policy.toml"))
    );
    assert_eq!(
        args.log_socket.as_deref(),
        Some(Path::new("/run/ultar/log/devd.sock"))
    );
    assert_eq!(args.config.keys, Path::new("/etc/ultar/trusted-keys"));
    assert!(args.allow_root && !args.allow_all && !args.once);
}

/// The soft limit `name` of this test process, as `/proc` shows it.
fn own_soft_limit(name: &str) -> String {
    let own = std::fs::read_to_string("/proc/self/limits").unwrap();
    let line = own.lines().find(|line| line.starts_with(name)).unwrap();
    line[name.len()..]
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}
