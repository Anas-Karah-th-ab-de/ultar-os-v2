//! Security review r3 (device identity) against the real server binary: what the log shows of a
//! complete device life: it must show no secret and no identifier. Passes on the reviewed code.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

const NAME: &str = "review.example";
const PASSWORD: &str = "Zx9-secret-passphrase";
const NEW_PASSWORD: &str = "Qv7-another-passphrase";

struct Server {
    child: Child,
    addr: String,
    log: Arc<Mutex<Vec<String>>>,
    _dir: tempfile::TempDir,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ultraos-server"))
        .env("ULTRAOS_DB", dir.path().join("db.sqlite"))
        .env("ULTRAOS_FILES", dir.path().join("files"))
        .env("ULTRAOS_SECRET", "cd".repeat(32))
        .env("ULTRAOS_BIND", "127.0.0.1:0")
        .env("ULTRAOS_SERVER_NAME", NAME)
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
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = log.clone();
    std::thread::spawn(move || {
        for line in lines.map_while(Result::ok) {
            sink.lock().unwrap().push(line);
        }
    });
    Server {
        child,
        addr,
        log,
        _dir: dir,
    }
}

fn post(server: &Server, path: &str, token: Option<&str>, body: &Value) -> (u16, Value) {
    let mut stream = TcpStream::connect(&server.addr).unwrap();
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let body = body.to_string();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(format!("{head}{body}").as_bytes())
        .unwrap();
    let mut out = String::new();
    let _ = stream.read_to_string(&mut out);
    let status = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let json = out.split("\r\n\r\n").nth(1).unwrap_or("");
    (status, serde_json::from_str(json).unwrap_or(Value::Null))
}

fn text(v: &Value, field: &str) -> String {
    v[field].as_str().unwrap_or_default().to_string()
}

fn device_id(public: &[u8; 32]) -> String {
    let digest = Sha256::new()
        .chain_update(b"ultar-device-v1")
        .chain_update(public)
        .finalize();
    let alphabet = b"abcdefghijklmnopqrstuvwxyz234567";
    let bits: String = digest[..20].iter().map(|b| format!("{b:08b}")).collect();
    (0..32)
        .map(|i| alphabet[usize::from_str_radix(&bits[i * 5..i * 5 + 5], 2).unwrap()] as char)
        .collect()
}

fn transcript(
    label: &str,
    account: i64,
    challenge: &[u8],
    key: &[u8; 32],
    extra: &[u8],
) -> Vec<u8> {
    let mut t = label.as_bytes().to_vec();
    t.push(0);
    t.extend_from_slice(&(NAME.len() as u16).to_be_bytes());
    t.extend_from_slice(NAME.as_bytes());
    t.extend_from_slice(&account.to_be_bytes());
    t.extend_from_slice(challenge);
    t.extend_from_slice(key);
    t.extend_from_slice(&(extra.len() as u16).to_be_bytes());
    t.extend_from_slice(extra);
    t
}

/// Everything secret or identifying that passed through the server in [`device_life`].
#[derive(Default)]
struct Seen(Vec<String>);

impl Seen {
    fn add(&mut self, value: &str) {
        if value.len() >= 8 {
            self.0.push(value.to_string());
        }
    }
}

/// Challenge + signature for `purpose`; records what went over the wire.
fn proof(
    server: &Server,
    seen: &mut Seen,
    key: &SigningKey,
    (purpose, label): (&str, &str),
    token: Option<&str>,
    extra: &[u8],
) -> (String, String) {
    let public = key.verifying_key().to_bytes();
    let ask = json!({"purpose": purpose, "public_key": B64.encode(public)});
    let (status, c) = post(server, "/v1/device/challenge", token, &ask);
    assert_eq!(status, 200, "{c}");
    let challenge = B64.decode(text(&c, "challenge")).unwrap();
    let account = c["account_id"].as_i64().unwrap();
    let signed = key.sign(&transcript(label, account, &challenge, &public, extra));
    let signature = B64.encode(signed.to_bytes());
    for value in [
        text(&c, "challenge"),
        text(&c, "challenge_id"),
        signature.clone(),
    ] {
        seen.add(&value);
    }
    (text(&c, "challenge_id"), signature)
}

async fn signal(server: &Server, seen: &mut Seen, key: &SigningKey) -> Value {
    let url = format!("ws://{}/v1/signal", server.addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let public = key.verifying_key().to_bytes();
    let hello = json!({"type": "hello", "device_id": device_id(&public)});
    ws.send(Message::Text(hello.to_string())).await.unwrap();
    let Some(Ok(Message::Text(frame))) = ws.next().await else {
        panic!("no challenge");
    };
    let c: Value = serde_json::from_str(&frame).unwrap();
    let challenge = B64.decode(text(&c, "challenge")).unwrap();
    let account = c["account_id"].as_i64().unwrap();
    let t = transcript("ultar-device-signal-v1", account, &challenge, &public, b"");
    let signature = B64.encode(key.sign(&t).to_bytes());
    seen.add(&text(&c, "challenge"));
    seen.add(&signature);
    let auth = json!({"type": "auth", "signature": signature});
    ws.send(Message::Text(auth.to_string())).await.unwrap();
    let Some(Ok(Message::Text(frame))) = ws.next().await else {
        panic!("no answer");
    };
    serde_json::from_str(&frame).unwrap()
}

const REGISTER: (&str, &str) = ("register", "ultar-device-register-v1");
const SESSION: (&str, &str) = ("session", "ultar-device-session-v1");
const RECOVER: (&str, &str) = ("recover", "ultar-device-recover-v1");

/// Registers `key`; returns its device token.
fn register(server: &Server, seen: &mut Seen, key: &SigningKey, token: &str) -> String {
    let public = key.verifying_key().to_bytes();
    let (id, sig) = proof(server, seen, key, REGISTER, Some(token), b"");
    let body = json!({"challenge_id": id, "signature": sig, "name": "Leak Probe Phone"});
    let (status, r) = post(server, "/v1/device/register", Some(token), &body);
    assert_eq!(status, 200, "{r}");
    seen.add(&text(&r, "number"));
    seen.add(&device_id(&public));
    seen.add(&B64.encode(public));
    seen.add(&hex::encode(public));
    text(&r, "token")
}

/// One refused and one accepted sign-in, then the signalling socket; returns the device token.
fn sign_in(server: &Server, seen: &mut Seen, key: &SigningKey) -> String {
    let (id, sig) = proof(server, seen, key, SESSION, None, b"");
    let wrong = json!({"challenge_id": id, "signature": B64.encode([7u8; 64])});
    assert_eq!(post(server, "/v1/device/session", None, &wrong).0, 401);
    seen.add(&sig);
    let (id, sig) = proof(server, seen, key, SESSION, None, b"");
    let body = json!({"challenge_id": id, "signature": sig});
    let (status, session) = post(server, "/v1/device/session", None, &body);
    assert_eq!(status, 200, "{session}");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    assert_eq!(rt.block_on(signal(server, seen, key))["type"], "ready");
    text(&session, "token")
}

/// A recovery attempt with a wrong and one with the right password.
fn recover(server: &Server, seen: &mut Seen, token: &str, lost: &str) {
    let taker = SigningKey::from_bytes(&[43; 32]);
    for password in ["Zx9-wrong-passphrase", PASSWORD] {
        let (id, sig) = proof(server, seen, &taker, RECOVER, Some(token), lost.as_bytes());
        let body = json!({
            "password": password,
            "old_device_id": lost,
            "challenge_id": id,
            "signature": sig,
        });
        post(server, "/v1/device/recover/start", Some(token), &body);
        seen.add(password);
    }
}

/// Account, two devices, sessions, socket, failed proofs, recovery, removal, password change.
fn device_life(server: &Server) -> Seen {
    let mut seen = Seen::default();
    let phone = SigningKey::from_bytes(&[41; 32]);
    let tablet = SigningKey::from_bytes(&[42; 32]);
    let account = json!({"username": "leak-probe-user", "password": PASSWORD});
    let (_, a) = post(server, "/v1/account/register", None, &account);
    let token = text(&a, "token");
    let mut tokens = vec![token.clone()];
    tokens.push(register(server, &mut seen, &phone, &token));
    tokens.push(register(server, &mut seen, &tablet, &token));
    let device_token = sign_in(server, &mut seen, &phone);
    let lost = device_id(&tablet.verifying_key().to_bytes());
    recover(server, &mut seen, &token, &lost);
    let named = json!({"device_id": lost});
    post(server, "/v1/device/remove", Some(&device_token), &named);
    let change = json!({"old_password": PASSWORD, "new_password": NEW_PASSWORD});
    let (status, changed) = post(server, "/v1/account/password", Some(&token), &change);
    assert_eq!(status, 200);
    tokens.push(device_token);
    tokens.push(text(&changed, "token"));
    for token in &tokens {
        seen.add(token);
        for half in token.split('.') {
            seen.add(half);
        }
    }
    for value in ["leak-probe-user", NEW_PASSWORD, "Leak Probe Phone"] {
        seen.add(value);
    }
    seen
}

#[test]
fn the_log_of_a_whole_device_life_shows_no_secret_and_no_identifier() {
    let server = start();
    let seen = device_life(&server);
    std::thread::sleep(Duration::from_millis(500));
    let log = server.log.lock().unwrap().clone();
    assert!(
        log.iter().any(|l| l.contains("device_registered")),
        "{log:?}"
    );
    // calls leave no trace: the signalling socket logs nothing
    assert!(
        !log.iter().any(|l| l.contains("signal_connected")),
        "{log:?}"
    );
    assert!(log.iter().any(|l| l.contains("recovery_started")));
    assert!(
        seen.0.len() > 30,
        "the probe recorded {} values",
        seen.0.len()
    );
    for line in &log {
        for value in &seen.0 {
            // A log line may name a device by at most the first 6 characters of its id.
            let probe = &value[..value.len().min(12)];
            assert!(!line.contains(probe), "log shows {probe:?}: {line}");
        }
        assert!(
            !line.contains("127.0.0.1") || line.contains("listening"),
            "{line}"
        );
    }
}
