//! A simulated device for the tests: an Ed25519 key, the transcripts of
//! `docs/DEVICE-IDENTITY.md` §4 built independently of the server code, and the HTTP / WebSocket
//! steps of the protocol.

use super::{Reply, TestServer};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_tungstenite::tungstenite::Message;

/// The server name of the test configuration (the default).
pub const SERVER: &str = "ultar.code-craft.tech";

pub type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A challenge as the server answered it.
#[derive(Debug, Clone)]
pub struct Challenge {
    pub id: String,
    pub bytes: [u8; 32],
    pub account_id: i64,
    pub server: String,
}

impl Challenge {
    pub fn from_json(v: &Value) -> Self {
        let bytes = B64.decode(v["challenge"].as_str().unwrap()).unwrap();
        Self {
            id: v["challenge_id"].as_str().unwrap_or_default().to_string(),
            bytes: bytes.try_into().unwrap(),
            account_id: v["account_id"].as_i64().unwrap(),
            server: v["server"].as_str().unwrap().to_string(),
        }
    }
}

/// Builds a transcript by hand (not with the server's code).
pub fn transcript(
    label: &str,
    server: &str,
    account_id: i64,
    challenge: &[u8; 32],
    subject: &[u8; 32],
    extra: &[u8],
) -> Vec<u8> {
    let mut t = label.as_bytes().to_vec();
    t.push(0);
    t.extend_from_slice(&(server.len() as u16).to_be_bytes());
    t.extend_from_slice(server.as_bytes());
    t.extend_from_slice(&account_id.to_be_bytes());
    t.extend_from_slice(challenge);
    t.extend_from_slice(subject);
    t.extend_from_slice(&(extra.len() as u16).to_be_bytes());
    t.extend_from_slice(extra);
    t
}

pub struct TestDevice {
    pub key: SigningKey,
}

impl TestDevice {
    pub fn new(seed: u8) -> Self {
        Self {
            key: SigningKey::from_bytes(&[seed; 32]),
        }
    }

    pub fn public(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    pub fn public_b64(&self) -> String {
        B64.encode(self.public())
    }

    /// The device id, computed independently: base32(SHA-256("ultar-device-v1" ‖ key)[..20]).
    pub fn id(&self) -> String {
        let mut h = Sha256::new();
        h.update(b"ultar-device-v1");
        h.update(self.public());
        let digest = h.finalize();
        let alphabet = b"abcdefghijklmnopqrstuvwxyz234567";
        let mut bits = String::new();
        for b in &digest[..20] {
            bits.push_str(&format!("{b:08b}"));
        }
        (0..32)
            .map(|i| alphabet[usize::from_str_radix(&bits[i * 5..i * 5 + 5], 2).unwrap()] as char)
            .collect()
    }

    pub fn challenge_reply(&self, s: &TestServer, purpose: &str, token: Option<&str>) -> Reply {
        s.post(
            "/v1/device/challenge",
            token,
            &json!({"purpose": purpose, "public_key": self.public_b64()}),
        )
    }

    pub fn challenge(&self, s: &TestServer, purpose: &str, token: Option<&str>) -> Challenge {
        let reply = self.challenge_reply(s, purpose, token);
        assert_eq!(reply.status, 200, "{}", reply.text());
        Challenge::from_json(&reply.json())
    }

    /// Signs the transcript `label` for challenge `c` (server name and account as the server sent them).
    pub fn sign(&self, label: &str, c: &Challenge, extra: &[u8]) -> String {
        self.sign_as(label, &c.server, c.account_id, &c.bytes, extra)
    }

    pub fn sign_as(
        &self,
        label: &str,
        server: &str,
        account_id: i64,
        challenge: &[u8; 32],
        extra: &[u8],
    ) -> String {
        let t = transcript(label, server, account_id, challenge, &self.public(), extra);
        B64.encode(self.key.sign(&t).to_bytes())
    }

    /// Full registration; returns the reply.
    pub fn register_reply(&self, s: &TestServer, token: &str, name: &str) -> Reply {
        let c = self.challenge(s, "register", Some(token));
        let signature = self.sign("ultar-device-register-v1", &c, b"");
        s.post(
            "/v1/device/register",
            Some(token),
            &json!({"challenge_id": c.id, "signature": signature, "name": name}),
        )
    }

    /// Full registration that must succeed; returns the answer.
    pub fn register(&self, s: &TestServer, token: &str) -> Value {
        let reply = self.register_reply(s, token, "test device");
        assert_eq!(reply.status, 200, "{}", reply.text());
        reply.json()
    }

    /// Registers and returns the number.
    pub fn number(&self, s: &TestServer, token: &str) -> String {
        self.register(s, token)["number"]
            .as_str()
            .unwrap()
            .to_string()
    }

    pub fn session_reply(&self, s: &TestServer) -> Reply {
        let reply = self.challenge_reply(s, "session", None);
        if reply.status != 200 {
            return reply;
        }
        let c = Challenge::from_json(&reply.json());
        let signature = self.sign("ultar-device-session-v1", &c, b"");
        s.post(
            "/v1/device/session",
            None,
            &json!({"challenge_id": c.id, "signature": signature}),
        )
    }

    /// A fresh device token.
    pub fn session(&self, s: &TestServer) -> String {
        let reply = self.session_reply(s);
        assert_eq!(reply.status, 200, "{}", reply.text());
        reply.json()["token"].as_str().unwrap().to_string()
    }

    /// Opens the signalling socket with the device key; returns the socket and the last frame of
    /// the handshake (`ready` or `error`).
    pub async fn signal(&self, s: &TestServer) -> (Socket, Value) {
        self.signal_signed_by(s, self).await
    }

    /// Like [`TestDevice::signal`], but the challenge is signed by `signer` (to test a wrong key).
    pub async fn signal_signed_by(&self, s: &TestServer, signer: &TestDevice) -> (Socket, Value) {
        let mut ws = open_signal(s).await;
        send_json(&mut ws, &json!({"type": "hello", "device_id": self.id()})).await;
        let first = next_json(&mut ws).await;
        if first["type"] != "challenge" {
            return (ws, first);
        }
        let challenge: [u8; 32] = B64
            .decode(first["challenge"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let t = transcript(
            "ultar-device-signal-v1",
            first["server"].as_str().unwrap(),
            first["account_id"].as_i64().unwrap(),
            &challenge,
            &self.public(),
            b"",
        );
        let signature = B64.encode(signer.key.sign(&t).to_bytes());
        send_json(&mut ws, &json!({"type": "auth", "signature": signature})).await;
        let last = next_json(&mut ws).await;
        (ws, last)
    }
}

pub async fn open_signal(s: &TestServer) -> Socket {
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/v1/signal", s.addr))
        .await
        .unwrap();
    ws
}

pub async fn send_json(ws: &mut Socket, value: &Value) {
    ws.send(Message::Text(value.to_string())).await.unwrap();
}

/// The next text frame as JSON; `{"closed":true}` when the socket ended.
pub async fn next_json(ws: &mut Socket) -> Value {
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(t))) => return serde_json::from_str(&t).unwrap(),
            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return json!({"closed": true}),
            Some(Ok(_)) => {}
        }
    }
}

pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
