//! Review round 3 (service runtime): regression tests for confirmed defects
//! of `ultar-busd`. Target path: `system/core/ultar-busd/tests/review3_busd.rs`.
//!
//! Every test marked `R3-…` FAILS on commit 157060b and must pass once the
//! defect named in its comment is fixed. The test marked "verified as
//! sound" passes today and stays as a guard.

mod common;

use common::{busd, connect, send_signal, temp_dir, WAIT};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Child;
use std::time::{Duration, Instant};
use ultar_bus::wire::{
    read_envelope, read_preamble, write_envelope, write_preamble, Envelope, Hello, Kind,
    PROTOCOL_VERSION,
};
use ultar_bus::{encode, Client, ClientOptions};

const POLICY: &str = r#"
[[rule]]
identity = "id.a"
subscribe = ["flood.*"]
[[rule]]
identity = "id.b"
publish = ["flood.data"]
[[rule]]
identity = "system.admin"
call = ["settings.*"]
"#;

/// Completes the handshake on a raw connection and returns it.
fn raw_session(socket: &Path) -> Option<UnixStream> {
    let mut stream = UnixStream::connect(socket).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write_preamble(&mut stream, PROTOCOL_VERSION).ok()?;
    read_preamble(&mut stream).ok()?;
    let hello = Hello {
        client_name: "r3".into(),
        token: None,
    };
    let hello = Envelope::new(Kind::Hello, 0).with_payload(PROTOCOL_VERSION, encode(&hello).ok()?);
    write_envelope(&mut stream, &hello, u32::MAX).ok()?;
    let welcome = read_envelope(&mut stream, 1 << 16).ok()?;
    (welcome.kind == Kind::Welcome).then_some(stream)
}

/// Starts the broker with three fixed-identity sockets as the shipped
/// service file does (one socket per identity, one policy).
fn three_sockets(dir: &Path) -> Child {
    let policy = dir.join("policy.toml");
    std::fs::write(&policy, POLICY).unwrap();
    let listen = |name: &str, id: &str| format!("{}={id}", dir.join(name).display());
    let child = busd()
        .args(["--listen", &listen("a.sock", "id.a")])
        .args(["--listen", &listen("b.sock", "id.b")])
        .args(["--listen", &listen("admin.sock", "system.admin")])
        .arg("--policy")
        .arg(&policy)
        .spawn()
        .unwrap();
    drop(connect(&dir.join("admin.sock"), "probe"));
    child
}

fn wait_exit(mut child: Child, limit: Duration) -> Option<i32> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code();
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// R3-B1: the per-listener connection bound (256) times three descriptors
/// per connection exceeds the broker's own RLIMIT_NOFILE (1024). Two
/// identities that stay inside *their* bounds lock out every other socket,
/// the administrator's included.
#[test]
fn two_identities_inside_their_bounds_must_not_lock_out_the_admin_socket() {
    let dir = temp_dir("r3-fd");
    let child = three_sockets(&dir);
    let mut held = Vec::new();
    for name in ["a.sock", "b.sock"] {
        for _ in 0..256 {
            if let Some(stream) = raw_session(&dir.join(name)) {
                held.push(stream);
            }
        }
    }
    let options = ClientOptions {
        connect_retry_for: Duration::from_secs(3),
        ..ClientOptions::named("admin")
    };
    let admin = Client::connect(&dir.join("admin.sock"), options);
    let admitted = held.len();
    send_signal(child.id(), "KILL");
    let _ = wait_exit(child, WAIT);
    assert!(
        admin.is_ok(),
        "admin locked out while {admitted} connections of two other identities are open"
    );
}

/// R3-B0 (critical): connections that say nothing are enough to end the
/// broker. Each one costs two threads; under the broker's own RLIMIT_AS
/// (1 GiB) the C library's per-thread arenas and the thread stacks use the
/// address space up after 190 to 300 connections (it varies with the
/// address space layout), the next thread start
/// panics inside `std` and the panic hook ends the process with code 70.
/// No handshake, no identity and no policy rule is needed.
#[test]
fn silent_connections_inside_the_listener_bound_must_not_end_the_broker() {
    let dir = temp_dir("r3-crash");
    let mut child = three_sockets(&dir);
    // 256 per socket: the bound of each listener, none exceeded.
    // Whether the failing allocation is a thread stack (refused, survived)
    // or the new thread's signal stack (panic) depends on the layout of
    // the address space: several rounds make the outcome certain.
    let mut died = None;
    for _ in 0..8 {
        let mut silent = Vec::new();
        // 256 per socket: the bound of each listener, none exceeded.
        for name in ["a.sock", "b.sock", "admin.sock"] {
            let path = dir.join(name);
            silent.extend((0..256).filter_map(|_| UnixStream::connect(&path).ok()));
        }
        std::thread::sleep(Duration::from_millis(300));
        died = child.try_wait().unwrap().and_then(|status| status.code());
        drop(silent);
        if died.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    send_signal_if_alive(&mut child);
    assert_eq!(died, None, "the broker ended by itself (70 = panic)");
}

fn send_signal_if_alive(child: &mut Child) {
    if child.try_wait().unwrap().is_none() {
        send_signal(child.id(), "KILL");
        let _ = child.wait();
    }
}

/// Verified as sound (passes): a stop must end with exit code 0 and without
/// socket files, also
/// when peers are connected that have not finished their handshake.
#[test]
fn sigterm_with_silent_peers_exits_cleanly_and_removes_the_sockets() {
    let dir = temp_dir("r3-term");
    let child = three_sockets(&dir);
    let silent: Vec<UnixStream> = (0..8)
        .map(|_| UnixStream::connect(dir.join("a.sock")).unwrap())
        .collect();
    std::thread::sleep(Duration::from_millis(100));
    send_signal(child.id(), "TERM");
    let code = wait_exit(child, WAIT);
    drop(silent);
    let left: Vec<&str> = ["a.sock", "b.sock", "admin.sock"]
        .into_iter()
        .filter(|name| dir.join(name).exists())
        .collect();
    assert_eq!(code, Some(0), "exit code after SIGTERM");
    assert!(left.is_empty(), "socket files left behind: {left:?}");
}

/// R3-B3: the token file is read as one "secret" of at most 4096 bytes, so
/// a file for more than about 60 clients is refused and the broker does
/// not start.
#[test]
fn a_token_file_for_a_hundred_clients_is_accepted() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("r3-tokens");
    let tokens = dir.join("tokens");
    let text: String = (0..100)
        .map(|n| format!("app.client{n} = \"0123456789abcdef0123456789abcdef-{n:03}\"\n"))
        .collect();
    std::fs::write(&tokens, text).unwrap();
    std::fs::set_permissions(&tokens, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut child = busd()
        .arg("--listen-token")
        .arg(dir.join("bus.sock"))
        .arg("--token-file")
        .arg(&tokens)
        .arg("--allow-all")
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(800));
    let ended = child.try_wait().unwrap().and_then(|status| status.code());
    send_signal_if_alive(&mut child);
    assert_eq!(ended, None, "the broker refused the token file and ended");
}
