//! The real `ultar-busd` binary: signals, socket handling, limits read
//! back, peer identity, logging of refusals, policy reload, and retained
//! topics after a broker restart.

mod common;

use common::{busd, connect, send_signal, temp_dir, terminate, text_field, LogSink, WAIT};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::Path;
use ultar_bus::{BusError, Client, ClientOptions, ErrorCode, Message, Topic};
use ultar_service::exit;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
struct Status {
    value: u32,
}

impl Message for Status {
    const VERSION: u16 = 1;
}

impl Topic for Status {
    const TOPIC: &'static str = "test.status";
    const RETAINED: bool = true;
}

fn write_private(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn sigterm_ends_with_code_zero_and_removes_the_socket() {
    let dir = temp_dir("term");
    let socket = dir.join("bus.sock");
    let child = busd().arg(&socket).arg("--allow-all").spawn().unwrap();
    let client = connect(&socket, "t");
    assert_eq!(client.identity(), "system");
    assert!(socket.exists());
    let (code, stderr) = terminate(child);
    assert_eq!(code, Some(exit::OK), "{stderr}");
    assert!(stderr.contains("System bus listening on:"), "{stderr}");
    assert!(stderr.ends_with("The system bus stopped.\n"), "{stderr}");
    assert!(!socket.exists(), "socket file must be removed");
}

#[test]
fn sigint_stops_too_and_clients_learn_it() {
    let dir = temp_dir("int");
    let socket = dir.join("bus.sock");
    let child = busd().arg(&socket).arg("--allow-all").spawn().unwrap();
    let options = ClientOptions {
        connect_retry_for: WAIT,
        reconnect: false,
        ..ClientOptions::named("t")
    };
    let client = Client::connect(&socket, options).unwrap();
    let events = client.subscribe_pattern("x.*").unwrap();
    send_signal(child.id(), "INT");
    // The subscription ends: the broker closed the connection in order.
    assert!(events.recv_timeout(WAIT).is_err());
    assert_eq!(child.wait_with_output().unwrap().status.code(), Some(0));
    assert!(!socket.exists());
}

#[test]
fn after_sigkill_the_stale_socket_is_replaced_but_a_file_is_never_removed() {
    let dir = temp_dir("stale");
    let socket = dir.join("bus.sock");
    let mut first = busd().arg(&socket).arg("--allow-all").spawn().unwrap();
    drop(connect(&socket, "t"));
    send_signal(first.id(), "KILL");
    assert_eq!(first.wait().unwrap().code(), None);
    assert!(std::fs::symlink_metadata(&socket)
        .unwrap()
        .file_type()
        .is_socket());
    // Restart: the dead socket is replaced and the broker serves again.
    let second = busd().arg(&socket).arg("--allow-all").spawn().unwrap();
    let client = connect(&socket, "t");
    assert!(client.is_connected());
    assert_eq!(terminate(second).0, Some(0));

    // Something that is not a socket at the path stays untouched.
    let occupied = dir.join("data.sock");
    std::fs::write(&occupied, b"precious").unwrap();
    let output = busd().arg(&occupied).arg("--allow-all").output().unwrap();
    assert_eq!(output.status.code(), Some(exit::FAILURE));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not a socket"), "{stderr}");
    assert_eq!(std::fs::read(&occupied).unwrap(), b"precious");
    // A symbolic link to a file is not followed and not removed either.
    let link = dir.join("link.sock");
    std::os::unix::fs::symlink(&occupied, &link).unwrap();
    let output = busd().arg(&link).arg("--allow-all").output().unwrap();
    assert_eq!(output.status.code(), Some(exit::FAILURE));
    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read(&occupied).unwrap(), b"precious");
}

#[test]
fn a_second_broker_on_a_live_socket_is_refused() {
    let dir = temp_dir("live");
    let socket = dir.join("bus.sock");
    let first = busd().arg(&socket).arg("--allow-all").spawn().unwrap();
    drop(connect(&socket, "t"));
    let output = busd().arg(&socket).arg("--allow-all").output().unwrap();
    assert_eq!(output.status.code(), Some(exit::FAILURE));
    // The first broker and its socket are unharmed.
    assert!(connect(&socket, "t").is_connected());
    assert_eq!(terminate(first).0, Some(0));
}

#[test]
fn limits_are_applied_to_the_running_broker() {
    let dir = temp_dir("limits");
    let socket = dir.join("bus.sock");
    let child = busd().arg(&socket).arg("--allow-all").spawn().unwrap();
    drop(connect(&socket, "t"));
    let limits = std::fs::read_to_string(format!("/proc/{}/limits", child.id())).unwrap();
    let own = std::fs::read_to_string("/proc/self/limits").unwrap();
    let row_of = |limits: &str, name: &str| -> Vec<String> {
        let line = limits.lines().find(|l| l.starts_with(name)).unwrap();
        line[name.len()..]
            .split_whitespace()
            .take(2)
            .map(str::to_owned)
            .collect()
    };
    let row = |name: &str| row_of(&limits, name);
    assert_eq!(row("Max open files"), ["1024", "1024"]);
    // No address-space limit of its own (review round 3, R3-B0): inherited.
    let space = "Max address space";
    assert_eq!(row(space), row_of(&own, space));
    assert_eq!(row("Max core file size"), ["0", "0"]);
    assert_eq!(row("Max processes"), ["1024", "1024"]);
    let status = std::fs::read_to_string(format!("/proc/{}/status", child.id())).unwrap();
    assert!(status
        .lines()
        .any(|l| l.starts_with("NoNewPrivs:") && l.ends_with('1')));
    let cwd = std::fs::read_link(format!("/proc/{}/cwd", child.id())).unwrap();
    assert_eq!(cwd, Path::new("/"));
    assert_eq!(terminate(child).0, Some(0));
}

#[test]
fn root_must_be_declared() {
    if ultar_service::effective_uid() != 0 {
        return;
    }
    let dir = temp_dir("root");
    let socket = dir.join("bus.sock");
    let output = std::process::Command::new(common::BUSD)
        .args(["--lang", "en", "--allow-all"])
        .arg(&socket)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(exit::HARDENING));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not run as root"), "{stderr}");
    assert!(!socket.exists());
}

#[test]
fn socket_mode_owner_and_group_are_set() {
    let dir = temp_dir("mode");
    let (private, shared) = (dir.join("private.sock"), dir.join("shared.sock"));
    let uid = ultar_service::effective_uid().to_string();
    let gid = ultar_service::group_id().to_string();
    let child = busd()
        .args(["--listen", &format!("{}=system", private.display())])
        .args(["--mode", "0600", "--owner", &uid, "--group", &gid])
        .args(["--listen", &format!("{}=app.any", shared.display())])
        .args(["--mode", "666", "--allow-all"])
        .spawn()
        .unwrap();
    assert_eq!(connect(&private, "t").identity(), "system");
    assert_eq!(connect(&shared, "t").identity(), "app.any");
    let meta = std::fs::metadata(&private).unwrap();
    assert_eq!(meta.mode() & 0o777, 0o600);
    assert_eq!(meta.uid().to_string(), uid);
    assert_eq!(meta.gid().to_string(), gid);
    assert_eq!(std::fs::metadata(&shared).unwrap().mode() & 0o777, 0o666);
    assert_eq!(terminate(child).0, Some(0));
    assert!(!private.exists() && !shared.exists());
}

#[test]
fn without_a_mode_the_socket_is_private_by_the_umask() {
    let dir = temp_dir("umask");
    let socket = dir.join("bus.sock");
    let child = busd().arg(&socket).arg("--allow-all").spawn().unwrap();
    drop(connect(&socket, "t"));
    assert_eq!(std::fs::metadata(&socket).unwrap().mode() & 0o077, 0);
    assert_eq!(terminate(child).0, Some(0));
}

#[test]
fn kernel_credentials_give_the_mapped_identity_and_refuse_everyone_else() {
    let dir = temp_dir("cred");
    let socket = dir.join("bus.sock");
    let map = dir.join("uids.toml");
    let uid = ultar_service::effective_uid();
    std::fs::write(&map, format!("# test\n{uid} = \"system.tester\"\n")).unwrap();
    let child = busd()
        .arg("--listen-cred")
        .arg(&socket)
        .args(["--mode", "666", "--uid-map"])
        .arg(&map)
        .arg("--allow-all")
        .spawn()
        .unwrap();
    // The name the client claims and a token it sends change nothing.
    let options = ClientOptions {
        connect_retry_for: WAIT,
        token: Some("i-am-root".into()),
        ..ClientOptions::named("system.settingsd")
    };
    let client = Client::connect(&socket, options).unwrap();
    assert_eq!(client.identity(), "system.tester");
    assert_eq!(terminate(child).0, Some(0));

    // The same user without entry: refused, with a log record.
    std::fs::write(
        &map,
        format!("{} = \"system.other\"\n", uid.wrapping_add(1)),
    )
    .unwrap();
    let sink = LogSink::start(&dir.join("log.sock"));
    let child = busd()
        .arg("--listen-cred")
        .arg(&socket)
        .arg("--uid-map")
        .arg(&map)
        .arg("--log-socket")
        .arg(&sink.path)
        .arg("--allow-all")
        .spawn()
        .unwrap();
    sink.expect("service.ready");
    let refused = Client::connect(&socket, ClientOptions::named("t"));
    assert!(matches!(refused, Err(BusError::AuthFailed)));
    sink.expect("bus.auth_failed");
    assert_eq!(terminate(child).0, Some(0));
}

#[test]
fn a_faulty_or_writable_uid_map_stops_the_start() {
    let dir = temp_dir("badmap");
    let socket = dir.join("bus.sock");
    let map = dir.join("uids.toml");
    let run = || {
        busd()
            .arg("--listen-cred")
            .arg(&socket)
            .arg("--uid-map")
            .arg(&map)
            .arg("--allow-all")
            .output()
            .unwrap()
    };
    std::fs::write(&map, "0 = system\n").unwrap();
    let output = run();
    assert_eq!(output.status.code(), Some(exit::FAILURE));
    assert!(String::from_utf8_lossy(&output.stderr).contains("map rejected"));
    std::fs::write(&map, "0 = \"system\"\n").unwrap();
    std::fs::set_permissions(&map, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert_eq!(run().status.code(), Some(exit::FAILURE));
    std::fs::remove_file(&map).unwrap();
    assert_eq!(run().status.code(), Some(exit::FAILURE));
    assert!(!socket.exists());
}

#[test]
fn tokens_come_from_a_private_file_never_from_the_command_line() {
    let dir = temp_dir("token");
    let socket = dir.join("bus.sock");
    let tokens = dir.join("tokens");
    write_private(
        &tokens,
        "system.settingsd = \"0123456789abcdef-settings\"\n",
    );
    let start = || {
        busd()
            .arg("--listen-token")
            .arg(&socket)
            .arg("--token-file")
            .arg(&tokens)
            .arg("--allow-all")
            .spawn()
            .unwrap()
    };
    let child = start();
    let with = |token: &str| {
        let options = ClientOptions {
            connect_retry_for: WAIT,
            token: Some(token.to_owned()),
            ..ClientOptions::named("t")
        };
        Client::connect(&socket, options)
    };
    assert_eq!(
        with("0123456789abcdef-settings").unwrap().identity(),
        "system.settingsd"
    );
    assert!(matches!(
        with("0123456789abcdef-settingX"),
        Err(BusError::AuthFailed)
    ));
    assert_eq!(terminate(child).0, Some(0));

    std::fs::set_permissions(&tokens, std::fs::Permissions::from_mode(0o644)).unwrap();
    let output = start().wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(exit::FAILURE));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Token file rejected: secret.too_permissive"),
        "{stderr}"
    );
    assert!(!stderr.contains("0123456789abcdef"));

    let output = busd()
        .arg(&socket)
        .args(["--allow-all", "--token", "abc"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(exit::USAGE));
}

#[test]
fn policy_and_allow_all_together_are_refused() {
    let dir = temp_dir("conflict");
    let output = busd()
        .arg(dir.join("bus.sock"))
        .args(["--allow-all", "--policy", "/nonexistent"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(exit::USAGE));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("--policy and --allow-all cannot be combined."),
        "{stderr}"
    );
    let neither = busd().arg(dir.join("bus.sock")).output().unwrap();
    assert_eq!(neither.status.code(), Some(exit::USAGE));
}

const DENY_PUBLISH: &str = "[[rule]]\nidentity = \"system\"\nsubscribe = [\"*\"]\n";
const ALLOW_PUBLISH: &str =
    "[[rule]]\nidentity = \"system\"\nsubscribe = [\"*\"]\npublish = [\"test.*\"]\n";

#[test]
fn refusals_and_limits_are_logged_and_sighup_reloads_the_policy() {
    let dir = temp_dir("log");
    let socket = dir.join("bus.sock");
    let policy = dir.join("policy.toml");
    std::fs::write(&policy, DENY_PUBLISH).unwrap();
    let sink = LogSink::start(&dir.join("log.sock"));
    let child = busd()
        .arg(&socket)
        .arg("--policy")
        .arg(&policy)
        .arg("--log-socket")
        .arg(&sink.path)
        .spawn()
        .unwrap();
    sink.expect("service.starting");
    sink.expect("bus.listening");
    sink.expect("service.ready");
    let client = connect(&socket, "t");

    let denied = client.publish(&Status { value: 1 });
    assert!(matches!(
        denied,
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
    let record = sink.expect("bus.refused");
    assert_eq!(text_field(&record, "op"), Some("publish"));
    assert_eq!(text_field(&record, "code"), Some("permission_denied"));
    assert_eq!(text_field(&record, "peer"), Some("system"));

    // The subscription limit of one connection (256).
    let subscriptions: Vec<_> = (0..256)
        .map(|n| client.subscribe_pattern(&format!("a.t{n}")).unwrap())
        .collect();
    let over = client.subscribe_pattern("a.over");
    assert!(matches!(
        over,
        Err(BusError::Remote(ErrorCode::LimitExceeded))
    ));
    let record = sink.expect("bus.limit_hit");
    assert_eq!(text_field(&record, "op"), Some("subscribe"));
    drop(subscriptions);

    // A broken policy file keeps the old policy; a good one replaces it.
    std::fs::write(&policy, "[[rule]\nbroken").unwrap();
    send_signal(child.id(), "HUP");
    sink.expect("bus.policy_reload_rejected");
    assert!(client.publish(&Status { value: 2 }).is_err());
    std::fs::write(&policy, ALLOW_PUBLISH).unwrap();
    send_signal(child.id(), "HUP");
    sink.expect("bus.policy_reloaded");
    client.publish(&Status { value: 3 }).unwrap();

    send_signal(child.id(), "TERM");
    sink.expect("service.stopping");
    sink.expect("bus.stopping");
    sink.expect("service.stopped");
    assert_eq!(child.wait_with_output().unwrap().status.code(), Some(0));
}
