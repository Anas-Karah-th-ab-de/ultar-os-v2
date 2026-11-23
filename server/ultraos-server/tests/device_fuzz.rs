//! Fixed-seed fuzzing of every new request body and of the signalling handshake: hostile input
//! never crashes the server, never produces a server error, and never changes stored state.

mod common;
use common::device::{next_json, open_signal, runtime, send_json, TestDevice};
use common::TestServer;
use serde_json::{json, Value};

/// xorshift64*: deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }

    fn text(&mut self) -> String {
        const ALPHABET: &[&str] = &[
            "a", "Z", "0", "-", "_", "=", "+", "/", ".", ":", " ", "\n", "\0", "\"", "'", "\\",
            "%", "{", "}", "[", "]", "ي", "ß", "😀", "\u{202e}", "legacy:", "UOS-", "../",
        ];
        let len = *self.pick(&[0u64, 1, 2, 7, 22, 32, 43, 44, 64, 86, 87, 129, 300, 2000]);
        (0..len).map(|_| *self.pick(ALPHABET)).collect()
    }

    fn b64(&mut self) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
        let len = *self.pick(&[0u64, 1, 15, 16, 17, 31, 32, 33, 63, 64, 65, 200]);
        let bytes: Vec<u8> = (0..len).map(|_| self.next() as u8).collect();
        B64.encode(bytes)
    }

    fn value(&mut self, depth: u32) -> Value {
        match self.below(if depth > 2 { 8 } else { 10 }) {
            0 => Value::Null,
            1 => json!(self.below(2) == 0),
            2 => json!(self.next() as i64),
            3 => json!(-1),
            4 => json!(1e308),
            5 => json!(self.text()),
            6 => json!(self.b64()),
            7 => json!(""),
            8 => Value::Array((0..self.below(4)).map(|_| self.value(depth + 1)).collect()),
            _ => {
                let mut map = serde_json::Map::new();
                for _ in 0..self.below(4) {
                    let key = self.text();
                    map.insert(key, self.value(depth + 1));
                }
                Value::Object(map)
            }
        }
    }
}

const FIELDS: &[&str] = &[
    "purpose",
    "public_key",
    "challenge_id",
    "signature",
    "name",
    "device_id",
    "pubkey",
    "legacy_device_id",
    "old_device_id",
    "old_signature",
    "new_signature",
    "password",
    "old_password",
    "new_password",
    "after",
    "type",
    "token",
    "number",
];
const PURPOSES: &[&str] = &[
    "register", "session", "upgrade", "replace", "recover", "signal", "", "REGISTER",
];

/// A body that looks like a request: known field names, values that are valid, nearly valid or junk.
fn body(rng: &mut Rng, known: &[String]) -> Value {
    let mut map = serde_json::Map::new();
    for _ in 0..rng.below(8) {
        let field = *rng.pick(FIELDS);
        let value = match rng.below(6) {
            0 => json!(rng.pick(known)),
            1 => json!(rng.b64()),
            2 => json!(*rng.pick(PURPOSES)),
            3 => json!(rng.text()),
            _ => rng.value(0),
        };
        map.insert(field.to_string(), value);
    }
    Value::Object(map)
}

const PATHS: &[&str] = &[
    "/v1/device/challenge",
    "/v1/device/register",
    "/v1/device/session",
    "/v1/device/upgrade",
    "/v1/device/rename",
    "/v1/device/remove",
    "/v1/device/replace",
    "/v1/device/recover/start",
    "/v1/device/recover/cancel",
    "/v1/device/recover/finish",
    "/v1/account/password",
];

fn snapshot(s: &TestServer) -> Vec<String> {
    let conn = s.state.db.lock();
    let mut out = Vec::new();
    for sql in [
        "SELECT id || ':' || account_id || ':' || device_id || ':' || state || ':' || epoch || ':' || name FROM devices ORDER BY id",
        "SELECT number || ':' || device_row || ':' || state FROM numbers ORDER BY number",
        "SELECT id || ':' || pw_hash || ':' || token_epoch FROM accounts ORDER BY id",
        "SELECT account_id || ':' || old_row FROM device_recoveries",
    ] {
        let mut stmt = conn.prepare(sql).unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
        out.extend(rows.map(Result::unwrap));
    }
    out
}

/// Sends one hostile request with one of the attacker's tokens and checks that the answer is a
/// typed client error. Returns the status.
fn hostile_request(
    s: &TestServer,
    rng: &mut Rng,
    token: &str,
    device_token: &str,
    known: &[String],
) -> u16 {
    let path = *rng.pick(PATHS);
    let auth = match rng.below(4) {
        0 => None,
        1 => Some(token),
        2 => Some(device_token),
        _ => Some("garbage.token"),
    };
    let payload = match rng.below(10) {
        0 => rng.value(0).to_string(),
        1 => rng.text(),
        2 => "{".repeat(rng.below(3000) as usize),
        _ => body(rng, known).to_string(),
    };
    let mut headers = vec![("content-type", "application/json")];
    let bearer = auth.map(|t| format!("Bearer {t}"));
    if let Some(b) = &bearer {
        headers.push(("authorization", b));
    }
    let reply = common::request("POST", &s.url(path), &headers, Some(&payload));
    assert!(
        matches!(
            reply.status,
            400 | 401 | 403 | 404 | 409 | 410 | 413 | 426 | 429
        ),
        "{path} {payload:?} -> {} {}",
        reply.status,
        reply.text()
    );
    let v = reply.json();
    assert!(v["error"].is_string(), "typed error body: {}", reply.text());
    reply.status
}

#[test]
fn hostile_bodies_never_break_the_server_or_change_state() {
    let s = TestServer::start();
    // The victim's account with two devices; the attacker's account with one device of its own.
    // The attacker sends everything he can know about the victim, with his own valid tokens.
    let victim = s.register("anna");
    let device = TestDevice::new(1);
    let registered = device.register(&s, &victim);
    TestDevice::new(2).register(&s, &victim);
    let token = s.register("mallory");
    let device_token = TestDevice::new(50).register(&s, &token)["token"]
        .as_str()
        .unwrap()
        .to_string();
    let known = vec![
        device.id(),
        TestDevice::new(2).id(),
        device.public_b64(),
        TestDevice::new(3).public_b64(),
        registered["number"].as_str().unwrap().to_string(),
        registered["token"].as_str().unwrap().to_string(),
        "AAAAAAAAAAAAAAAAAAAAAA".to_string(),
        "A".repeat(86),
        format!("legacy:{}", device.id()),
    ];
    let before = snapshot(&s);
    let mut rng = Rng(0x5EED_0015_D0D0_CAFE);
    let mut seen = std::collections::BTreeMap::<u16, u32>::new();
    for round in 0..1100u32 {
        if round % 6 == 0 {
            s.clock.advance_millis(120_000); // let fuzz-made challenges expire
        }
        let status = hostile_request(&s, &mut rng, &token, &device_token, &known);
        *seen.entry(status).or_default() += 1;
    }
    assert_eq!(
        snapshot(&s),
        before,
        "no fuzzed request changed stored state"
    );
    assert!(
        seen.len() >= 4,
        "the fuzzer reached several code paths: {seen:?}"
    );
    // The server still serves the real protocol.
    s.clock.advance_millis(120_000);
    assert_eq!(s.get("/v1/health").status, 200);
    assert_eq!(device.session_reply(&s).status, 200);
}

#[test]
fn hostile_challenge_requests_yield_challenges_only_for_valid_keys() {
    let s = TestServer::start();
    let token = s.register("anna");
    let mut rng = Rng(0xC0FF_EE15_0000_0001);
    let good = TestDevice::new(1).public_b64();
    let mut issued = 0;
    for round in 0..300u32 {
        if round % 4 == 0 {
            s.clock.advance_millis(120_000);
        }
        let key = if rng.below(4) == 0 {
            good.clone()
        } else {
            rng.b64()
        };
        let purpose = *rng.pick(PURPOSES);
        let reply = s.post(
            "/v1/device/challenge",
            Some(&token),
            &json!({"purpose": purpose, "public_key": key}),
        );
        match reply.status {
            200 => {
                issued += 1;
                assert!(ed25519_dalek::VerifyingKey::from_bytes(
                    &base64::Engine::decode(
                        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                        &key
                    )
                    .unwrap()
                    .try_into()
                    .unwrap()
                )
                .is_ok());
                assert!(["register", "upgrade", "replace", "recover"].contains(&purpose));
            }
            400 | 404 | 410 | 429 => assert!(reply.json()["error"].is_string()),
            other => panic!("{purpose} {key} -> {other}"),
        }
    }
    assert!(issued > 5, "{issued}");
    assert!(
        s.state.devices.challenges.len() <= 8,
        "per-account bound held throughout"
    );
}

#[test]
fn hostile_signalling_frames_never_authenticate() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(1);
    let number = device.number(&s, &token);
    let known = vec![
        device.id(),
        number.clone(),
        token.clone(),
        device.public_b64(),
        "A".repeat(86),
    ];
    runtime().block_on(async {
        let mut rng = Rng(0x0515_0515_0515_0515);
        for _ in 0..120 {
            let mut ws = open_signal(&s).await;
            let mut first = body(&mut rng, &known);
            if rng.below(2) == 0 {
                first["type"] = json!("hello");
                first["device_id"] = json!(device.id());
                first.as_object_mut().unwrap().remove("token");
            }
            send_json(&mut ws, &first).await;
            let mut answer = next_json(&mut ws).await;
            if answer["type"] == "challenge" {
                let mut second = body(&mut rng, &known);
                if rng.below(2) == 0 {
                    second["type"] = json!("auth");
                    second["signature"] = json!(rng.b64());
                }
                send_json(&mut ws, &second).await;
                answer = next_json(&mut ws).await;
            }
            assert_eq!(answer["type"], "error", "{first} -> {answer}");
            assert!(!s.state.hub.is_online(&number));
        }
        let (_ws, ready) = device.signal(&s).await;
        assert_eq!(ready["type"], "ready");
    });
}
