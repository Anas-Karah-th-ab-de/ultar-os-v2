//! The settings daemon on the service runtime: stop signals flush pending
//! writes, a kill is recovered from, limits are applied, the token comes
//! from a file, the retained store report returns after a broker restart,
//! and the two access options cannot be combined.

use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::settings::{Get, Set, SettingValue, StoreReport};
use ultar_bus::{
    Broker, Client, ClientOptions, FixedIdentity, Identity, PeerIdentity, Policy, TokenIdentity,
};
use ultar_settingsd::testdir::TestDir;

const DAEMON: &str = env!("CARGO_BIN_EXE_ultar-settingsd");
const WAIT: Duration = Duration::from_secs(30);
/// A write delay that never expires during a test: what reaches the disk
/// got there because the daemon flushed while stopping.
const NEVER: &str = "60000";

fn send_signal(pid: u32, signal: &str) {
    let status = Command::new("kill")
        .args([&format!("-{signal}"), &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success());
}

fn broker(socket: &Path, auth: Arc<dyn PeerIdentity>) -> Broker {
    Broker::builder(Policy::allow_all())
        .listen(socket, auth)
        .start()
        .unwrap()
}

fn fixed() -> Arc<dyn PeerIdentity> {
    Arc::new(FixedIdentity::new(Identity::new("test.peer").unwrap()))
}

fn daemon(dir: &Path, extra: &[&str]) -> Command {
    let mut command = Command::new(DAEMON);
    command
        .arg("--store")
        .arg(dir.join("store"))
        .arg("--socket")
        .arg(dir.join("bus.sock"))
        .args(["--user", "anas", "--lang", "en", "--write-delay-ms", NEVER])
        .args(extra)
        .env_remove("ULTAR_BUS_SOCKET")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if ultar_service::effective_uid() == 0 {
        command.arg("--allow-root");
    }
    command
}

/// Starts the daemon and waits for its ready line.
fn start(dir: &Path, extra: &[&str]) -> Child {
    let mut child = daemon(dir, extra).spawn().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "Settings service ready");
    child
}

fn client(dir: &Path) -> Client {
    Client::connect(&dir.join("bus.sock"), ClientOptions::named("test")).unwrap()
}

fn set_theme(client: &Client, theme: &str) {
    let set = Set {
        key: "appearance.theme".into(),
        value: SettingValue::Text(theme.into()),
    };
    assert!(client.call(&set, WAIT).unwrap().changed);
}

fn user_file(dir: &Path) -> PathBuf {
    dir.join("store/user-anas.toml")
}

#[test]
fn sigterm_writes_the_pending_change_and_exits_with_zero() {
    let dir = TestDir::new("rt-term");
    let _broker = broker(&dir.path().join("bus.sock"), fixed());
    let child = start(dir.path(), &["--allow-all"]);
    let bus = client(dir.path());
    set_theme(&bus, "light");
    // Answered, but with a write delay of a minute not on disk yet.
    let before = std::fs::read_to_string(user_file(dir.path())).unwrap_or_default();
    assert!(!before.contains("appearance.theme"), "{before}");
    send_signal(child.id(), "TERM");
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let after = std::fs::read_to_string(user_file(dir.path())).unwrap();
    assert!(
        after.contains("\"appearance.theme\" = \"light\""),
        "{after}"
    );
}

#[test]
fn sigint_does_the_same() {
    let dir = TestDir::new("rt-int");
    let _broker = broker(&dir.path().join("bus.sock"), fixed());
    let child = start(dir.path(), &["--allow-all"]);
    set_theme(&client(dir.path()), "light");
    send_signal(child.id(), "INT");
    assert_eq!(child.wait_with_output().unwrap().status.code(), Some(0));
    let after = std::fs::read_to_string(user_file(dir.path())).unwrap();
    assert!(
        after.contains("\"appearance.theme\" = \"light\""),
        "{after}"
    );
}

#[test]
fn after_sigkill_the_daemon_starts_again_and_serves() {
    let dir = TestDir::new("rt-kill");
    let _broker = broker(&dir.path().join("bus.sock"), fixed());
    // First life: a change that is flushed by an orderly stop.
    let child = start(dir.path(), &["--allow-all"]);
    set_theme(&client(dir.path()), "light");
    send_signal(child.id(), "TERM");
    assert_eq!(child.wait_with_output().unwrap().status.code(), Some(0));
    // Second life: killed with a change still pending.
    let mut child = start(dir.path(), &["--allow-all"]);
    let bus = client(dir.path());
    let set = Set {
        key: "appearance.font_size".into(),
        value: SettingValue::Integer(120),
    };
    assert!(bus.call(&set, WAIT).unwrap().changed);
    send_signal(child.id(), "KILL");
    assert_eq!(child.wait().unwrap().code(), None);
    // Third life: starts (the name `settings` is free again once the
    // broker has noticed the dead connection), the flushed value is
    // there, the file is intact. The unflushed change is lost – that is
    // what a write delay means – but nothing is damaged.
    let child = start(dir.path(), &["--allow-all"]);
    let bus = client(dir.path());
    let get = Get {
        key: "appearance.theme".into(),
    };
    let reply = bus.call(&get, WAIT).unwrap();
    assert_eq!(reply.value, Some(SettingValue::Text("light".into())));
    assert!(!dir.path().join("store/quarantine").exists());
    send_signal(child.id(), "TERM");
    assert_eq!(child.wait_with_output().unwrap().status.code(), Some(0));
}

#[test]
fn limits_are_applied_and_root_must_be_declared() {
    let dir = TestDir::new("rt-limits");
    let _broker = broker(&dir.path().join("bus.sock"), fixed());
    let child = start(dir.path(), &["--allow-all"]);
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
    // A build run by its owner shares that user with cargo and the other tests: the limit
    // on the user's processes stays as inherited (ultar-service, `Outcome::SharedUser`).
    assert!(
        has("Max processes", &own_soft_limit("Max processes")),
        "{limits}"
    );
    send_signal(child.id(), "TERM");
    assert_eq!(child.wait_with_output().unwrap().status.code(), Some(0));

    if ultar_service::effective_uid() == 0 {
        let output = Command::new(DAEMON)
            .arg("--store")
            .arg(dir.path().join("store"))
            .arg("--socket")
            .arg(dir.path().join("bus.sock"))
            .args(["--allow-all", "--lang", "en"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(71));
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "The settings service does not run as root without --allow-root\n"
        );
    }
}

#[test]
fn policy_together_with_allow_all_is_refused() {
    let dir = TestDir::new("rt-conflict");
    let policy = dir.path().join("policy.toml");
    std::fs::write(&policy, "[[rule]]\nidentity = \"x\"\n").unwrap();
    let output = daemon(
        dir.path(),
        &["--allow-all", "--policy", policy.to_str().unwrap()],
    )
    .output()
    .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("--policy and --allow-all cannot be combined\nUsage: ultar-settingsd"),
        "{stderr}"
    );
}

#[test]
fn the_token_comes_from_a_private_file_and_never_from_the_command_line() {
    let dir = TestDir::new("rt-token");
    let auth = TokenIdentity::new()
        .with_token(
            "settings-token-0123456789",
            Identity::new("system.settingsd").unwrap(),
        )
        .with_token(
            "panel-token-0123456789",
            Identity::new("app.settings").unwrap(),
        );
    let _broker = broker(&dir.path().join("bus.sock"), Arc::new(auth));
    let token = dir.path().join("token");
    std::fs::write(&token, "settings-token-0123456789\n").unwrap();
    let chmod = |mode| {
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    chmod(0o600);
    let token_arg = token.to_str().unwrap();
    let child = start(dir.path(), &["--allow-all", "--token-file", token_arg]);
    // The token is not on the command line of the running process.
    let cmdline = std::fs::read(format!("/proc/{}/cmdline", child.id())).unwrap();
    assert!(!String::from_utf8_lossy(&cmdline).contains("settings-token"));
    let options = ClientOptions {
        token: Some("panel-token-0123456789".into()),
        ..ClientOptions::named("panel")
    };
    let panel = Client::connect(&dir.path().join("bus.sock"), options).unwrap();
    let get = Get {
        key: "appearance.theme".into(),
    };
    assert!(panel.call(&get, WAIT).unwrap().value.is_some());
    send_signal(child.id(), "TERM");
    assert_eq!(child.wait_with_output().unwrap().status.code(), Some(0));

    // A token file others can read is refused, without echoing the token.
    chmod(0o644);
    let output = daemon(dir.path(), &["--allow-all", "--token-file", token_arg])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.ends_with("The token could not be read: secret.too_permissive\n"),
        "{stderr}"
    );
    assert!(!stderr.contains("0123456789"));
    // `--token` does not exist.
    let output = daemon(dir.path(), &["--allow-all", "--token", "abc"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn the_store_report_is_published_again_after_a_broker_restart() {
    let dir = TestDir::new("rt-retained");
    let socket = dir.path().join("bus.sock");
    // A damaged user file makes the daemon publish a (retained) report.
    std::fs::create_dir_all(dir.path().join("store")).unwrap();
    std::fs::write(user_file(dir.path()), "appearance = broken").unwrap();
    let first = broker(&socket, fixed());
    let child = start(dir.path(), &["--allow-all"]);
    let early = client(dir.path());
    let report = early
        .subscribe::<StoreReport>()
        .unwrap()
        .recv_timeout(WAIT)
        .unwrap();
    assert_eq!(report.notices.len(), 1);
    // The broker goes away and comes back with empty memory.
    drop(first);
    let _second = broker(&socket, fixed());
    let late = client(dir.path());
    let again = late
        .subscribe::<StoreReport>()
        .unwrap()
        .recv_timeout(WAIT)
        .unwrap();
    assert_eq!(again, report);
    // The service is registered again as well.
    let get = Get {
        key: "appearance.theme".into(),
    };
    let reply = loop {
        match late.call(&get, WAIT) {
            Ok(reply) => break reply,
            Err(ultar_bus::BusError::Remote(ultar_bus::ErrorCode::UnknownService)) => {
                std::thread::yield_now();
            }
            Err(other) => panic!("{other}"),
        }
    };
    assert!(reply.value.is_some());
    send_signal(child.id(), "TERM");
    assert_eq!(child.wait_with_output().unwrap().status.code(), Some(0));
}

#[test]
fn operation_is_logged_through_the_log_socket_not_standard_error() {
    let dir = TestDir::new("rt-log");
    let _broker = broker(&dir.path().join("bus.sock"), fixed());
    let log_socket = dir.path().join("log.sock");
    let listener = UnixListener::bind(&log_socket).unwrap();
    let mut child = daemon(
        dir.path(),
        &["--allow-all", "--log-socket", log_socket.to_str().unwrap()],
    )
    .spawn()
    .unwrap();
    let (mut stream, _) = listener.accept().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    send_signal(child.id(), "TERM");
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let mut keys = Vec::new();
    ultar_service::log::wire::FrameReader::new()
        .push(&bytes, |body| {
            keys.push(ultar_service::log::wire::decode_body(body).unwrap().key);
        })
        .unwrap();
    // What hardening could not do on this kernel or was told not to do
    // (root declared) is logged too; it depends on who runs the tests.
    assert!(keys
        .iter()
        .any(|key| key == "service.hardening.unsupported"));
    keys.retain(|key| !key.starts_with("service.hardening."));
    assert_eq!(
        keys,
        [
            "service.starting",
            "settingsd.allow_all",
            "service.ready",
            "service.stopping",
            "settingsd.flushed",
            "service.stopped"
        ]
    );
    // Standard error carries the start-up warning for the operator only.
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "Warning: everyone who reaches the service counts as administrator (--allow-all)\n"
    );
}

#[test]
fn the_shipped_service_definition_is_a_command_line_the_daemon_accepts() {
    use ultar_settingsd::daemon::{parse, DaemonCommand};
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../rootfs/etc/ultar/services/settingsd.toml");
    let table: toml::Table = std::fs::read_to_string(file).unwrap().parse().unwrap();
    let args: Vec<String> = table["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap().to_owned())
        .collect();
    let DaemonCommand::Run(args) = parse(args, None).unwrap() else {
        panic!("the definition must run the daemon");
    };
    assert_eq!(args.store, Path::new("/data/ultar/settings"));
    assert_eq!(args.socket, Path::new("/run/ultar/bus/settingsd.sock"));
    assert_eq!(
        args.policy.as_deref(),
        Some(Path::new("/etc/ultar/bus-policy.toml"))
    );
    assert_eq!(
        args.log_socket.as_deref(),
        Some(Path::new("/run/ultar/log/settingsd.sock"))
    );
    // Started as its own user by ultar-init: root is neither needed nor declared.
    assert!(!args.allow_root && args.token_file.is_none() && args.token_fd.is_none());
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
