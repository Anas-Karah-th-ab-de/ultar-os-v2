//! Security review: abuse of the public endpoints that the existing tests do not cover.
//! Starts the real server binary on an ephemeral port. A failing test is a finding.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};

struct Server {
    child: Child,
    addr: String,
    _dir: tempfile::TempDir,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(general_limit: &str) -> Server {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ultraos-server"))
        .env("ULTRAOS_DB", dir.path().join("db.sqlite"))
        .env("ULTRAOS_FILES", dir.path().join("files"))
        .env("ULTRAOS_SECRET", "ab".repeat(32))
        .env("ULTRAOS_BIND", "127.0.0.1:0")
        .env("ULTRAOS_LIMIT_GENERAL", general_limit)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let addr = loop {
        let line = lines.next().expect("server ended").unwrap();
        if let Some(i) = line.find("addr=") {
            break line[i + 5..].trim().trim_matches('"').to_string();
        }
    };
    std::thread::spawn(move || for _ in lines {});
    Server {
        child,
        addr,
        _dir: dir,
    }
}

/// One request through "the proxy" (we are on loopback, so X-Forwarded-For names the client).
fn post(
    server: &Server,
    path: &str,
    body: &str,
    token: Option<&str>,
    client: &str,
) -> (u16, String) {
    let mut s = TcpStream::connect(&server.addr).unwrap();
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let req = format!("POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nX-Forwarded-For: {client}\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let status = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (
        status,
        out.split("\r\n\r\n").nth(1).unwrap_or("").to_string(),
    )
}

fn token_of(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string()
}

fn peak_rss_kib(pid: u32) -> u64 {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    let line = status.lines().find(|l| l.starts_with("VmHWM:")).unwrap();
    line.split_whitespace().nth(1).unwrap().parse().unwrap()
}

/// Unauthenticated clients must not decide how much memory the server uses: every password hash costs
/// about 19 MiB while it is computed, so the number of concurrent computations has to be bounded.
#[test]
fn concurrent_registrations_do_not_multiply_memory() {
    let server = std::sync::Arc::new(start("120,120"));
    let before = peak_rss_kib(server.child.id());
    let threads: Vec<_> = (0..300)
        .map(|i| {
            let server = server.clone();
            std::thread::spawn(move || {
                post(
                    &server,
                    "/v1/account/register",
                    &format!(r#"{{"username":"flood-{i}","password":"12345678"}}"#),
                    None,
                    &format!("100.64.{}.{}", i / 250, i % 250 + 1),
                )
                .0
            })
        })
        .collect();
    let ok = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .filter(|c| *c == 200)
        .count();
    let after = peak_rss_kib(server.child.id());
    assert_eq!(ok, 300, "every client stayed inside its own limit");
    assert!(
        after - before < 400 * 1024,
        "300 concurrent registrations from 300 addresses raised the server's memory by {} MiB",
        (after - before) / 1024
    );
}

/// One account must not be able to use up the address pool that all accounts share.
#[test]
fn one_account_cannot_take_many_vpn_addresses() {
    let server = start("1000000,1000000");
    let token = token_of(
        &post(
            &server,
            "/v1/account/register",
            r#"{"username":"mallory","password":"12345678"}"#,
            None,
            "198.51.100.1",
        )
        .1,
    );
    let mut granted = 0;
    for i in 0..200u32 {
        let mut key = [0u8; 32];
        key[..4].copy_from_slice(&i.to_le_bytes());
        let key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, key);
        if post(
            &server,
            "/v1/vpn/peer",
            &format!(r#"{{"pubkey":"{key}"}}"#),
            Some(&token),
            "198.51.100.1",
        )
        .0 == 200
        {
            granted += 1;
        }
    }
    assert!(granted <= 32, "one account was given {granted} addresses of the shared 10.80.0.0/16 pool (nothing stops it at 65 533)");
}

/// The login answer must not tell by its duration whether the user exists.
#[test]
fn login_timing_does_not_reveal_accounts() {
    let server = start("1000000,1000000");
    post(
        &server,
        "/v1/account/register",
        r#"{"username":"alice","password":"12345678"}"#,
        None,
        "198.51.100.1",
    );
    let median = |user: &str, block: u32| {
        let mut t: Vec<_> = (0..7u32)
            .map(|i| {
                let start = std::time::Instant::now();
                let (code, _) = post(
                    &server,
                    "/v1/account/login",
                    &format!(r#"{{"username":"{user}","password":"wrong-password"}}"#),
                    None,
                    &format!("203.0.{block}.{}", i + 1),
                );
                assert_eq!(code, 401);
                start.elapsed()
            })
            .collect();
        t.sort();
        t[3]
    };
    let (known, unknown) = (median("alice", 1), median("nobody", 2));
    assert!(known < unknown * 3, "wrong password for an existing account takes {known:?}, for an unknown account {unknown:?}");
}

fn get_status(server: &Server, path: &str, token: &str) -> (u16, String) {
    let mut s = TcpStream::connect(&server.addr).unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\nX-Forwarded-For: 198.51.100.9\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    (
        out.split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0),
        out.split("\r\n\r\n").nth(1).unwrap_or("").to_string(),
    )
}

/// A token of an account the operator removed must stop working, and must never speak for the account
/// that is registered next (SQLite gives the freed account number out again).
#[test]
fn token_of_a_deleted_account_is_dead_and_does_not_become_another_account() {
    let server = start("1000000,1000000");
    let old = token_of(
        &post(
            &server,
            "/v1/account/register",
            r#"{"username":"banned","password":"12345678"}"#,
            None,
            "198.51.100.1",
        )
        .1,
    );
    assert_eq!(get_status(&server, "/v1/account/entitlements", &old).0, 200);
    let db = rusqlite::Connection::open(server._dir.path().join("db.sqlite")).unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON; DELETE FROM accounts WHERE username='banned';")
        .unwrap();
    assert_eq!(
        get_status(&server, "/v1/account/entitlements", &old).0,
        401,
        "token of a deleted account is still accepted"
    );
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let new = post(
        &server,
        "/v1/account/register",
        r#"{"username":"newcustomer","password":"12345678"}"#,
        None,
        "198.51.100.2",
    )
    .1;
    let new_id = serde_json::from_str::<serde_json::Value>(&new).unwrap()["account_id"]
        .as_i64()
        .unwrap();
    db.execute(
        "INSERT INTO entitlements(account_id, kind, item, granted) VALUES(?1,'channel','lts',0)",
        [new_id],
    )
    .unwrap();
    let (status, body) = get_status(&server, "/v1/account/entitlements", &old);
    assert!(
        status == 401 && !body.contains("lts"),
        "the deleted account's token now acts as the new account: {status} {body}"
    );
    assert_eq!(
        get_status(&server, "/v1/account/entitlements", &token_of(&new)).0,
        200,
        "control: the new account's own token works"
    );
}

/// Like [`start`], but keeps every log line the server writes.
fn start_logged() -> (Server, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ultraos-server"))
        .env("ULTRAOS_DB", dir.path().join("db.sqlite"))
        .env("ULTRAOS_FILES", dir.path().join("files"))
        .env("ULTRAOS_SECRET", "ab".repeat(32))
        .env("ULTRAOS_BIND", "127.0.0.1:0")
        .env("ULTRAOS_LIMIT_GENERAL", "1000000,1000000")
        .env("ULTRAOS_SERVER_NAME", "Review.Example")
        .env("ULTRAOS_MAX_DEVICES", "2")
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let addr = loop {
        let line = lines.next().expect("server ended").unwrap();
        if let Some(i) = line.find("addr=") {
            break line[i + 5..].trim().trim_matches('"').to_string();
        }
    };
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = log.clone();
    std::thread::spawn(move || {
        for line in lines.map_while(Result::ok) {
            sink.lock().unwrap().push(line);
        }
    });
    let server = Server {
        child,
        addr,
        _dir: dir,
    };
    (server, log)
}

/// Registers `key` for the account of `token` with a proof of possession; returns status and body.
fn register_key(server: &Server, token: &str, key: &ed25519_dalek::SigningKey) -> (u16, String) {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
    use ed25519_dalek::Signer;
    let public = key.verifying_key().to_bytes();
    let (status, body) = post(
        server,
        "/v1/device/challenge",
        &format!(
            r#"{{"purpose":"register","public_key":"{}"}}"#,
            B64.encode(public)
        ),
        Some(token),
        "198.51.100.7",
    );
    assert_eq!(status, 200, "{body}");
    let c: serde_json::Value = serde_json::from_str(&body).unwrap();
    let server_name = c["server"].as_str().unwrap();
    let mut t = b"ultar-device-register-v1\0".to_vec();
    t.extend_from_slice(&(server_name.len() as u16).to_be_bytes());
    t.extend_from_slice(server_name.as_bytes());
    t.extend_from_slice(&c["account_id"].as_i64().unwrap().to_be_bytes());
    t.extend_from_slice(&B64.decode(c["challenge"].as_str().unwrap()).unwrap());
    t.extend_from_slice(&public);
    t.extend_from_slice(&[0, 0]);
    post(
        server,
        "/v1/device/register",
        &format!(
            r#"{{"challenge_id":"{}","signature":"{}"}}"#,
            c["challenge_id"].as_str().unwrap(),
            B64.encode(key.sign(&t).to_bytes())
        ),
        Some(token),
        "198.51.100.7",
    )
}

/// Finding S15: "whoever sends a device id first owns it; without an Android serial number all
/// devices share one constant id". Against the real binary: sending an id claims nothing, two
/// serial-less devices are two devices, a key cannot be taken into another account, the device
/// limit of the environment holds, and the log never shows an id, a number or a key in full.
#[test]
fn a_device_id_cannot_be_squatted_and_logs_show_no_identifiers() {
    let (server, log) = start_logged();
    let account = |name: &str| {
        token_of(
            &post(
                &server,
                "/v1/account/register",
                &format!(r#"{{"username":"{name}","password":"12345678"}}"#),
                None,
                "198.51.100.1",
            )
            .1,
        )
    };
    let (mallory, victim) = (account("mallory"), account("victim"));
    let first = ed25519_dalek::SigningKey::from_bytes(&[41; 32]);
    let (one, two) = squatting_attempts_fail(&server, &mallory, &victim, &first);
    log_shows_no_identifiers(&server, &log, &first, &one, &two);
}

/// First half of the test above; returns the answers to the victim's two registrations.
fn squatting_attempts_fail(
    server: &Server,
    mallory: &str,
    victim: &str,
    first: &ed25519_dalek::SigningKey,
) -> (serde_json::Value, serde_json::Value) {
    // The constant id of serial-less devices under the first release, and an id of her choosing.
    for id in ["7af8c5b4c52e91a3", "victims-device"] {
        let (status, body) = post(
            server,
            "/v1/device/register",
            &format!(r#"{{"device_id":"{id}","pubkey":""}}"#),
            Some(mallory),
            "198.51.100.1",
        );
        assert!(
            status == 426 && body.contains("device_key_required"),
            "sending an id still registers something: {status} {body}"
        );
    }
    let second = ed25519_dalek::SigningKey::from_bytes(&[42; 32]);
    let (status, body) = register_key(server, victim, first);
    assert_eq!(status, 200, "the victim's device is blocked: {body}");
    let one: serde_json::Value = serde_json::from_str(&body).unwrap();
    let (status, body) = register_key(server, victim, &second);
    assert_eq!(status, 200, "{body}");
    let two: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_ne!(one["device_id"], two["device_id"], "devices share an id");
    assert_ne!(one["number"], two["number"]);
    // The victim's key under Mallory's account, with a valid signature for her account.
    let (status, body) = register_key(server, mallory, first);
    assert!(
        status == 409 && body.contains("device_taken"),
        "{status} {body}"
    );
    // ULTRAOS_MAX_DEVICES=2
    let third = ed25519_dalek::SigningKey::from_bytes(&[43; 32]);
    let (status, body) = register_key(server, victim, &third);
    assert!(
        status == 409 && body.contains("device_limit"),
        "{status} {body}"
    );
    (one, two)
}

/// Second half of the test above.
fn log_shows_no_identifiers(
    server: &Server,
    log: &std::sync::Mutex<Vec<String>>,
    first: &ed25519_dalek::SigningKey,
    one: &serde_json::Value,
    two: &serde_json::Value,
) {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
    // Give the log thread the lines of the last request: a health request is logged after them.
    post(server, "/v1/health-marker", "{}", None, "198.51.100.1");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !log
        .lock()
        .unwrap()
        .iter()
        .any(|l| l.contains("health-marker"))
    {
        assert!(
            std::time::Instant::now() < deadline,
            "log line did not arrive"
        );
        std::thread::yield_now();
    }
    let text = log.lock().unwrap().join("\n");
    assert!(
        text.contains("device_registered"),
        "control: registrations are logged"
    );
    for secret in [
        one["device_id"].as_str().unwrap(),
        two["device_id"].as_str().unwrap(),
        one["number"].as_str().unwrap(),
        one["token"].as_str().unwrap(),
        &B64.encode(first.verifying_key().to_bytes()),
        "7af8c5b4c52e91a3",
    ] {
        assert!(!text.contains(secret), "the log contains {secret}");
    }
    let prefix = &one["device_id"].as_str().unwrap()[..6];
    assert!(text.contains(prefix), "control: the short prefix is logged");
}
