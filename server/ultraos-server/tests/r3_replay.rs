//! Security review r3 (device identity, finding S15): replay, transplanting and cross-protocol
//! reuse of signed transcripts, and malformed Ed25519 inputs. Every test here is an attack that
//! must fail; a failing test is a finding.

mod common;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use common::device::{next_json, open_signal, runtime, send_json, Challenge, TestDevice};
use common::TestServer;
use serde_json::{json, Value};

// Adapted after review round 3: two labels were added (end of a recovery, approval).
const LABELS: [&str; 9] = [
    "ultar-device-register-v1",
    "ultar-device-session-v1",
    "ultar-device-signal-v1",
    "ultar-device-upgrade-v1",
    "ultar-device-replace-old-v1",
    "ultar-device-replace-new-v1",
    "ultar-device-recover-v1",
    "ultar-device-recover-finish-v1",
    "ultar-device-approve-v1",
];

fn code(reply: &common::Reply) -> (u16, String) {
    (reply.status, reply.error_code())
}

fn register_with(s: &TestServer, token: &str, id: &str, signature: &str) -> (u16, String) {
    let body = json!({"challenge_id": id, "signature": signature, "name": "x"});
    code(&s.post("/v1/device/register", Some(token), &body))
}

fn device_rows(s: &TestServer) -> i64 {
    s.state
        .db
        .lock()
        .query_row("SELECT COUNT(*) FROM devices", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn a_register_proof_cannot_be_replayed_or_moved_to_another_challenge() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(1);
    let first = device.challenge(&s, "register", Some(&token));
    let sig = device.sign(LABELS[0], &first, b"");
    assert_eq!(register_with(&s, &token, &first.id, &sig).0, 200);
    // Replay of the very same request.
    assert_eq!(
        register_with(&s, &token, &first.id, &sig),
        (400, "challenge_used".into())
    );
    // The captured signature under a fresh challenge of the same key and account.
    let second = device.challenge(&s, "register", Some(&token));
    assert_eq!(
        register_with(&s, &token, &second.id, &sig),
        (401, "signature_invalid".into())
    );
    // The failed attempt used the challenge up: even the right signature is too late.
    let late = device.sign(LABELS[0], &second, b"");
    assert_eq!(
        register_with(&s, &token, &second.id, &late),
        (400, "challenge_used".into())
    );
    assert_eq!(device_rows(&s), 1);
}

#[test]
fn a_proof_made_for_one_account_registers_nothing_in_another() {
    let s = TestServer::start();
    let (anna, mallory) = (s.register("anna"), s.register("mallory"));
    let device = TestDevice::new(2);
    // Mallory replays Anna's whole request (challenge id + signature) with her own token.
    let c = device.challenge(&s, "register", Some(&anna));
    let sig = device.sign(LABELS[0], &c, b"");
    assert_eq!(
        register_with(&s, &mallory, &c.id, &sig),
        (400, "challenge_mismatch".into())
    );
    assert_eq!(device_rows(&s), 0);
    // Mallory gets her own challenge for Anna's key and replays a signature Anna's device made
    // for Anna's account over the same key: the account id in the transcript differs.
    let hers = device.challenge(&s, "register", Some(&mallory));
    let for_anna = device.sign_as(LABELS[0], &hers.server, c.account_id, &hers.bytes, b"");
    assert_ne!(hers.account_id, c.account_id);
    assert_eq!(
        register_with(&s, &mallory, &hers.id, &for_anna),
        (401, "signature_invalid".into())
    );
    assert_eq!(device_rows(&s), 0);
}

#[test]
fn a_signature_that_names_another_server_is_useless_here() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(3);
    // A malicious server relays this server's challenge; the device signs for the name it is
    // configured with (the malicious one).
    for name in [
        "evil.example",
        "ultar.code-craft.tech.",
        "ultar.code-craft.tech:443",
        "",
    ] {
        let c = device.challenge(&s, "register", Some(&token));
        let sig = device.sign_as(LABELS[0], name, c.account_id, &c.bytes, b"");
        assert_eq!(
            register_with(&s, &token, &c.id, &sig),
            (401, "signature_invalid".into()),
            "{name:?}"
        );
    }
    assert_eq!(device_rows(&s), 0);
    // And a second server with another name refuses what was signed for the first.
    let other = TestServer::start_with(|c| c.devices.server_name = "other.example".into());
    let token2 = other.register("anna");
    let c = device.challenge(&other, "register", Some(&token2));
    assert_eq!(c.server, "other.example");
    let sig = device.sign_as(
        LABELS[0],
        common::device::SERVER,
        c.account_id,
        &c.bytes,
        b"",
    );
    assert_eq!(
        register_with(&other, &token2, &c.id, &sig),
        (401, "signature_invalid".into())
    );
}

/// One endpoint that takes a proof: path, challenge purpose, its own label, the `extra` of the
/// transcript and the rest of the body.
struct Target {
    path: &'static str,
    purpose: &'static str,
    label: &'static str,
    extra: Vec<u8>,
    body: Value,
}

fn targets(victim: &TestDevice) -> Vec<Target> {
    let id = victim.id();
    let t = |path, purpose, label, extra: &[u8], body| Target {
        path,
        purpose,
        label,
        extra: extra.to_vec(),
        body,
    };
    vec![
        t("/v1/device/register", "register", LABELS[0], b"", json!({})),
        t("/v1/device/session", "session", LABELS[1], b"", json!({})),
        t(
            "/v1/device/upgrade",
            "upgrade",
            LABELS[3],
            b"old",
            json!({"legacy_device_id": "old"}),
        ),
        t(
            "/v1/device/recover/start",
            "recover",
            LABELS[6],
            id.as_bytes(),
            json!({"password": "password-123", "old_device_id": id}),
        ),
        t(
            "/v1/device/recover/finish",
            "recover",
            LABELS[7],
            id.as_bytes(),
            json!({"old_device_id": id}),
        ),
    ]
}

fn present(s: &TestServer, token: &str, t: &Target, c: &Challenge, sig: &str) -> (u16, String) {
    let mut body = t.body.clone();
    body["challenge_id"] = json!(c.id);
    body["signature"] = json!(sig);
    code(&s.post(t.path, Some(token), &body))
}

#[test]
fn a_signature_for_one_purpose_is_refused_for_every_other() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (victim, device) = (TestDevice::new(4), TestDevice::new(5));
    victim.register(&s, &token);
    device.register(&s, &token); // so that session challenges exist for this key
    let before = snapshot(&s);
    s.clock.advance_millis(121_000);
    for target in targets(&victim) {
        for label in LABELS.iter().filter(|l| **l != target.label) {
            let c = device.challenge(&s, target.purpose, Some(&token));
            let sig = device.sign(label, &c, &target.extra);
            assert_eq!(
                present(&s, &token, &target, &c, &sig),
                (401, "signature_invalid".into()),
                "{label} at {}",
                target.path
            );
        }
        s.clock.advance_millis(121_000); // let the per-account challenge budget refill
                                         // The right label, but a challenge that was issued for another purpose.
        for purpose in ["register", "session", "upgrade", "replace", "recover"] {
            if purpose == target.purpose {
                continue;
            }
            let c = device.challenge(&s, purpose, Some(&token));
            let sig = device.sign(target.label, &c, &target.extra);
            assert_eq!(
                present(&s, &token, &target, &c, &sig),
                (400, "challenge_mismatch".into()),
                "{purpose} challenge at {}",
                target.path
            );
            s.clock.advance_millis(121_000); // let the per-account challenge budget refill
        }
        s.clock.advance_millis(121_000);
    }
    assert_eq!(snapshot(&s), before, "no attempt changed anything");
}

fn snapshot(s: &TestServer) -> Vec<String> {
    let conn = s.state.db.lock();
    let mut out = Vec::new();
    for sql in [
        "SELECT account_id || ':' || device_id || ':' || state || ':' || epoch FROM devices ORDER BY id",
        "SELECT number || ':' || device_row || ':' || state FROM numbers ORDER BY number",
        "SELECT account_id || ':' || old_row FROM device_recoveries",
    ] {
        let mut stmt = conn.prepare(sql).unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
        out.extend(rows.map(Result::unwrap));
    }
    out
}

#[test]
fn http_proofs_do_not_open_the_signalling_socket_and_the_other_way_round() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(6);
    device.register(&s, &token);
    runtime().block_on(async {
        for label in LABELS.iter().filter(|l| **l != "ultar-device-signal-v1") {
            let mut ws = open_signal(&s).await;
            send_json(&mut ws, &json!({"type":"hello","device_id": device.id()})).await;
            let c = Challenge::from_json(&next_json(&mut ws).await);
            let sig = device.sign(label, &c, b"");
            send_json(&mut ws, &json!({"type":"auth","signature": sig})).await;
            assert_eq!(next_json(&mut ws).await["error"], "auth", "{label}");
        }
        // A socket challenge signed for the socket gives no device token: it is not in the store,
        // and a session challenge signed with the signalling label is refused.
        let c = device.challenge(&s, "session", None);
        let sig = device.sign("ultar-device-signal-v1", &c, b"");
        let body = json!({"challenge_id": c.id, "signature": sig});
        assert_eq!(
            code(&s.post("/v1/device/session", None, &body)),
            (401, "signature_invalid".into())
        );
    });
}

/// The eight points of small order, plus non-canonical encodings of two of them.
fn weak_keys() -> Vec<[u8; 32]> {
    let hex = [
        "0100000000000000000000000000000000000000000000000000000000000000",
        "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
        "0000000000000000000000000000000000000000000000000000000000000080",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a",
        "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa",
        "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05",
        "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc85",
        "0100000000000000000000000000000000000000000000000000000000000080",
        "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
    ];
    hex.iter()
        .map(|h| hex::decode(h).unwrap().try_into().unwrap())
        .collect()
}

#[test]
fn weak_and_malformed_public_keys_get_no_challenge() {
    let s = TestServer::start();
    let token = s.register("anna");
    for key in weak_keys() {
        for purpose in ["register", "session", "replace", "recover", "upgrade"] {
            let body = json!({"purpose": purpose, "public_key": B64.encode(key)});
            let reply = s.post("/v1/device/challenge", Some(&token), &body);
            assert_eq!(code(&reply), (400, "invalid_request".into()), "{key:?}");
        }
    }
    let good = TestDevice::new(7).public();
    for text in [
        B64.encode(&good[..31]),
        B64.encode([good.as_slice(), &[0]].concat()),
        format!("{}=", B64.encode(good)),
        base64::engine::general_purpose::STANDARD.encode([0xfb; 32]),
        "A".repeat(100_000 / 8),
    ] {
        let body = json!({"purpose": "register", "public_key": text});
        let reply = s.post("/v1/device/challenge", Some(&token), &body);
        assert_eq!(code(&reply), (400, "invalid_request".into()), "{text}");
    }
    assert!(s.state.devices.challenges.is_empty());
}

/// `s + L` for the group order L: the same signature in a non-canonical form.
fn malleated(signature: &[u8; 64]) -> Option<[u8; 64]> {
    const L: [u8; 32] = [
        0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde,
        0x14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
    ];
    let mut out = *signature;
    let mut carry = 0u16;
    for i in 0..32 {
        let sum = u16::from(signature[32 + i]) + u16::from(L[i]) + carry;
        out[32 + i] = (sum & 0xff) as u8;
        carry = sum >> 8;
    }
    (carry == 0).then_some(out)
}

#[test]
fn non_canonical_and_malformed_signatures_are_refused() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(8);
    let mut tried = 0;
    while tried < 3 {
        let c = device.challenge(&s, "register", Some(&token));
        let sig: [u8; 64] = B64
            .decode(device.sign(LABELS[0], &c, b""))
            .unwrap()
            .try_into()
            .unwrap();
        let Some(twin) = malleated(&sig) else {
            continue; // s + L does not fit 256 bits for this signature; take another challenge
        };
        tried += 1;
        assert_eq!(
            register_with(&s, &token, &c.id, &B64.encode(twin)),
            (401, "signature_invalid".into())
        );
    }
    for bad in [
        B64.encode([0u8; 64]),
        B64.encode([0xffu8; 64]),
        B64.encode([0u8; 63]),
        B64.encode([0u8; 65]),
        String::new(),
        "*".repeat(86),
    ] {
        let c = device.challenge(&s, "register", Some(&token));
        let (status, error) = register_with(&s, &token, &c.id, &bad);
        assert!(
            matches!(
                (status, error.as_str()),
                (401, "signature_invalid") | (400, "invalid_request")
            ),
            "{bad}: {status} {error}"
        );
        s.clock.advance_millis(121_000);
    }
    assert_eq!(device_rows(&s), 0);
}
