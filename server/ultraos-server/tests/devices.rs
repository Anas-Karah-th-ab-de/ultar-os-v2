//! Device registration with proof of possession (finding S15): happy path, every refusal, the
//! squatting scenario of the review, replay across accounts and servers, concurrency, limits,
//! the device list and what the server stores.

mod common;
use common::device::{Challenge, TestDevice, SERVER};
use common::TestServer;
use serde_json::{json, Value};

const REGISTER: &str = "ultar-device-register-v1";

fn register_with(
    s: &TestServer,
    token: &str,
    challenge_id: &str,
    signature: &str,
) -> common::Reply {
    s.post(
        "/v1/device/register",
        Some(token),
        &json!({"challenge_id": challenge_id, "signature": signature}),
    )
}

fn code(reply: &common::Reply) -> (u16, String) {
    (reply.status, reply.error_code())
}

fn device_count(s: &TestServer) -> i64 {
    s.state
        .db
        .lock()
        .query_row("SELECT COUNT(*) FROM devices", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn registration_proves_the_key_and_assigns_id_number_and_token() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(1);

    let c = device.challenge_reply(&s, "register", Some(&token)).json();
    assert_eq!(c["server"], SERVER);
    assert_eq!(c["account_id"], 1);
    assert_eq!(c["expires_in"], 120);
    let c = Challenge::from_json(&c);
    assert_eq!(c.id.len(), 22, "16 random bytes, base64url");

    let reply = register_with(&s, &token, &c.id, &device.sign(REGISTER, &c, b""));
    assert_eq!(reply.status, 200, "{}", reply.text());
    let v = reply.json();
    assert_eq!(
        v["device_id"],
        device.id().as_str(),
        "id is derived from the key"
    );
    assert_eq!(v["device_id"].as_str().unwrap().len(), 32);
    assert_eq!(v["account_id"], 1);
    assert_eq!(v["expires_in"], 86_400);
    let number = v["number"].as_str().unwrap();
    assert!(number.starts_with("UOS-") && number.len() == 13);

    // The device token works for the account and names this device.
    let device_token = v["token"].as_str().unwrap();
    let list = s.get_as("/v1/devices", device_token).json();
    assert_eq!(list["limit"], 10);
    assert_eq!(list["devices"].as_array().unwrap().len(), 1);
    let entry = &list["devices"][0];
    assert_eq!(entry["device_id"], device.id().as_str());
    assert_eq!(entry["number"], number);
    assert_eq!(
        (entry["state"].as_str(), entry["current"].as_bool()),
        (Some("active"), Some(true))
    );
    assert_eq!(entry["last_seen"].as_i64().unwrap() % 86_400, 0, "day only");
    assert_eq!(entry["added"].as_i64().unwrap() % 86_400, 0, "day only");
    assert_eq!(
        s.get_as("/v1/devices", &token).json()["devices"][0]["current"],
        false
    );

    // Registering the same key again (new challenge) returns the same number, no second row.
    assert_eq!(device.number(&s, &token), number);
    assert_eq!(device_count(&s), 1);
}

#[test]
fn every_wrong_proof_is_refused_with_its_own_error_and_registers_nothing() {
    let s = TestServer::start();
    let (anna, bert) = (s.register("anna"), s.register("bert"));
    let (device, other) = (TestDevice::new(1), TestDevice::new(2));

    wrong_signatures_are_refused(&s, &anna, &device, &other);
    foreign_challenges_are_refused(&s, &anna, &bert, &device);
    malformed_requests_and_replays_are_refused(&s, &anna, &device);
    assert_eq!(device_count(&s), 1);
}

fn wrong_signatures_are_refused(
    s: &TestServer,
    anna: &str,
    device: &TestDevice,
    other: &TestDevice,
) {
    // wrong key: the challenge was issued for `device`, signed by `other`
    let c = device.challenge(s, "register", Some(anna));
    let forged = other.sign_as(REGISTER, &c.server, c.account_id, &c.bytes, b"");
    assert_eq!(
        code(&register_with(s, anna, &c.id, &forged)),
        (401, "signature_invalid".into())
    );
    // …and that attempt used the challenge up: the right signature is too late now
    let good = device.sign(REGISTER, &c, b"");
    assert_eq!(
        code(&register_with(s, anna, &c.id, &good)),
        (400, "challenge_used".into())
    );

    // signature over another purpose, another account id, another challenge, with extra data
    for (label, account, bytes, extra) in [
        ("ultar-device-session-v1", 1, None, &b""[..]),
        (REGISTER, 2, None, &b""[..]),
        (REGISTER, 1, Some([7u8; 32]), &b""[..]),
        (REGISTER, 1, None, &b"x"[..]),
    ] {
        let c = device.challenge(s, "register", Some(anna));
        let sig = device.sign_as(label, &c.server, account, &bytes.unwrap_or(c.bytes), extra);
        assert_eq!(
            code(&register_with(s, anna, &c.id, &sig)),
            (401, "signature_invalid".into()),
            "{label} {account}"
        );
    }
}

fn foreign_challenges_are_refused(s: &TestServer, anna: &str, bert: &str, device: &TestDevice) {
    // wrong account: bert presents anna's challenge with a signature that is valid for anna
    let c = device.challenge(s, "register", Some(anna));
    let sig = device.sign(REGISTER, &c, b"");
    assert_eq!(
        code(&register_with(s, bert, &c.id, &sig)),
        (400, "challenge_mismatch".into())
    );
    assert_eq!(
        code(&register_with(s, anna, &c.id, &sig)),
        (400, "challenge_used".into())
    );

    // a challenge issued for another purpose
    let c = device.challenge(s, "replace", Some(anna));
    let sig = device.sign(REGISTER, &c, b"");
    assert_eq!(
        code(&register_with(s, anna, &c.id, &sig)),
        (400, "challenge_mismatch".into())
    );
}

fn malformed_requests_and_replays_are_refused(s: &TestServer, anna: &str, device: &TestDevice) {
    // unknown challenge id, malformed id, malformed signature, missing fields
    let c = device.challenge(s, "register", Some(anna));
    let sig = device.sign(REGISTER, &c, b"");
    let unknown = "AAAAAAAAAAAAAAAAAAAAAA";
    assert_eq!(
        code(&register_with(s, anna, unknown, &sig)),
        (400, "challenge_unknown".into())
    );
    assert_eq!(
        code(&register_with(s, anna, "short", &sig)),
        (400, "invalid_request".into())
    );
    assert_eq!(
        code(&register_with(s, anna, &c.id, "AAAA")),
        (400, "invalid_request".into())
    );
    let no_sig = s.post(
        "/v1/device/register",
        Some(anna),
        &json!({"challenge_id": c.id}),
    );
    assert_eq!(code(&no_sig), (400, "invalid_request".into()));
    // no token at all
    let anonymous = s.post(
        "/v1/device/register",
        None,
        &json!({"challenge_id": c.id, "signature": sig}),
    );
    assert_eq!(code(&anonymous), (401, "auth_required".into()));
    // the malformed attempts did not consume the challenge; it still works once
    assert_eq!(register_with(s, anna, &c.id, &sig).status, 200);
    assert_eq!(
        code(&register_with(s, anna, &c.id, &sig)),
        (400, "challenge_used".into()),
        "replay"
    );
}

#[test]
fn an_expired_challenge_is_refused() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(1);
    let c = device.challenge(&s, "register", Some(&token));
    let sig = device.sign(REGISTER, &c, b"");
    s.clock.advance_millis(120_000);
    assert_eq!(
        code(&register_with(&s, &token, &c.id, &sig)),
        (400, "challenge_expired".into())
    );
    let c = device.challenge(&s, "register", Some(&token));
    s.clock.advance_millis(119_999);
    let sig = device.sign(REGISTER, &c, b"");
    assert_eq!(register_with(&s, &token, &c.id, &sig).status, 200);
}

#[test]
fn challenge_requests_are_validated_and_bounded() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(1);
    let ask = |purpose: &str, key: &str, token: Option<&str>| {
        s.post(
            "/v1/device/challenge",
            token,
            &json!({"purpose": purpose, "public_key": key}),
        )
    };
    assert_eq!(
        code(&ask("register", &device.public_b64(), None)),
        (401, "auth_required".into())
    );
    assert_eq!(
        code(&ask("signal", &device.public_b64(), Some(&token))),
        (400, "invalid_request".into())
    );
    assert_eq!(
        code(&ask("register", "AAAA", Some(&token))),
        (400, "invalid_request".into())
    );
    // small-order key (the identity point)
    let weak = "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    assert_eq!(
        code(&ask("register", weak, Some(&token))),
        (400, "invalid_request".into())
    );
    // a session challenge exists only for a registered key
    assert_eq!(
        code(&ask("session", &device.public_b64(), None)),
        (404, "device_unknown".into())
    );

    // at most 8 pending challenges per account
    for _ in 0..8 {
        assert_eq!(
            ask("register", &device.public_b64(), Some(&token)).status,
            200
        );
    }
    let full = ask("register", &device.public_b64(), Some(&token));
    assert_eq!(code(&full), (429, "challenge_capacity".into()));
    assert_eq!(full.header("retry-after"), Some("120"));
    // another account is not affected; after the lifetime there is room again
    let bert = s.register("bert");
    assert_eq!(
        ask("register", &device.public_b64(), Some(&bert)).status,
        200
    );
    s.clock.advance_millis(120_000);
    assert_eq!(
        ask("register", &device.public_b64(), Some(&token)).status,
        200
    );
}

/// The reviewer's scenario: Mallory sends the victim's device id first. With key-derived ids she
/// cannot: she would need the victim's private key. And the form that allowed it creates nothing.
#[test]
fn squatting_a_device_id_no_longer_works() {
    let s = TestServer::start();
    let (mallory, victim_token) = (s.register("mallory"), s.register("victim"));
    let victim = TestDevice::new(1);
    let mallory_device = TestDevice::new(66);

    // 1. the old attack: send the id (and anything as "pubkey") before the victim does
    for id in [
        victim.id(),
        "7af8c5b4c52e91a3".to_string(),
        "unknown".to_string(),
    ] {
        let r = s.post(
            "/v1/device/register",
            Some(&mallory),
            &json!({"device_id": id, "pubkey": victim.public_b64()}),
        );
        assert_eq!(code(&r), (426, "device_key_required".into()), "{id}");
    }
    // 2. claim the victim's public key and sign with her own key
    let c = victim.challenge(&s, "register", Some(&mallory));
    let forged = mallory_device.sign_as(REGISTER, &c.server, c.account_id, &c.bytes, b"");
    assert_eq!(
        code(&register_with(&s, &mallory, &c.id, &forged)),
        (401, "signature_invalid".into())
    );
    // 3. replay a signature the victim once made for his own account
    let c_victim = victim.challenge(&s, "register", Some(&victim_token));
    let victims_sig = victim.sign(REGISTER, &c_victim, b"");
    let c = victim.challenge(&s, "register", Some(&mallory));
    assert_eq!(
        code(&register_with(&s, &mallory, &c.id, &victims_sig)),
        (401, "signature_invalid".into())
    );
    assert_eq!(
        code(&register_with(&s, &mallory, &c_victim.id, &victims_sig)),
        (400, "challenge_mismatch".into())
    );
    assert_eq!(device_count(&s), 0, "nothing was registered by any attempt");

    // The victim registers afterwards and is not blocked.
    let v = victim.register(&s, &victim_token);
    assert_eq!(v["device_id"], victim.id().as_str());
    // Two devices without any serial number get different ids and numbers.
    let second = TestDevice::new(2).register(&s, &victim_token);
    assert_ne!(second["device_id"], v["device_id"]);
    assert_ne!(second["number"], v["number"]);
    // A key that is registered cannot be taken into another account, even with its private key.
    let r = victim.register_reply(&s, &mallory, "");
    assert_eq!(code(&r), (409, "device_taken".into()));
}

#[test]
fn a_signature_is_bound_to_one_server() {
    let a = TestServer::start_with(|c| c.devices.server_name = "a.example".into());
    let b = TestServer::start_with(|c| c.devices.server_name = "b.example".into());
    let (token_a, token_b) = (a.register("anna"), b.register("anna"));
    let device = TestDevice::new(1);

    // A challenge of server B relayed through "server A": the device signs for the name it knows (A).
    let c_b = device.challenge(&b, "register", Some(&token_b));
    assert_eq!(c_b.server, "b.example");
    let for_a = device.sign_as(REGISTER, "a.example", c_b.account_id, &c_b.bytes, b"");
    assert_eq!(
        code(&register_with(&b, &token_b, &c_b.id, &for_a)),
        (401, "signature_invalid".into())
    );

    // A complete, valid exchange recorded at A is worthless at B (same account id, same key).
    let c_a = device.challenge(&a, "register", Some(&token_a));
    let sig_a = device.sign(REGISTER, &c_a, b"");
    assert_eq!(register_with(&a, &token_a, &c_a.id, &sig_a).status, 200);
    assert_eq!(
        code(&register_with(&b, &token_b, &c_a.id, &sig_a)),
        (400, "challenge_unknown".into())
    );
    let c_b = device.challenge(&b, "register", Some(&token_b));
    assert_eq!(
        code(&register_with(&b, &token_b, &c_b.id, &sig_a)),
        (401, "signature_invalid".into())
    );
    assert_eq!(device_count(&b), 0);
}

#[test]
fn concurrent_registrations_of_one_key_end_with_one_device_and_one_number() {
    let s = std::sync::Arc::new(TestServer::start());
    // Eight accounts each hold a valid proof for the same key (the device signed for all of them).
    let device = std::sync::Arc::new(TestDevice::new(1));
    let proofs: Vec<(String, String, String)> = (0..8)
        .map(|i| {
            let token = s.register(&format!("user-{i}"));
            let c = device.challenge(&s, "register", Some(&token));
            let sig = device.sign(REGISTER, &c, b"");
            (token, c.id, sig)
        })
        .collect();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(proofs.len()));
    let threads: Vec<_> = proofs
        .into_iter()
        .map(|(token, id, sig)| {
            let (s, barrier) = (s.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let r = register_with(&s, &token, &id, &sig);
                (r.status, r.json())
            })
        })
        .collect();
    let results: Vec<(u16, Value)> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    let winners: Vec<&Value> = results
        .iter()
        .filter(|(c, _)| *c == 200)
        .map(|(_, v)| v)
        .collect();
    assert_eq!(winners.len(), 1, "{results:?}");
    assert!(results
        .iter()
        .filter(|(c, _)| *c != 200)
        .all(|(c, v)| *c == 409 && v["error"] == "device_taken"));
    let conn = s.state.db.lock();
    let (devices, numbers): (i64, i64) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM devices), (SELECT COUNT(*) FROM numbers)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((devices, numbers), (1, 1));
}

#[test]
fn concurrent_registrations_by_the_same_account_are_idempotent() {
    let s = std::sync::Arc::new(TestServer::start());
    let token = s.register("anna");
    let device = TestDevice::new(1);
    let proofs: Vec<(String, String)> = (0..6)
        .map(|_| {
            let c = device.challenge(&s, "register", Some(&token));
            let sig = device.sign(REGISTER, &c, b"");
            (c.id, sig)
        })
        .collect();
    let threads: Vec<_> = proofs
        .into_iter()
        .map(|(id, sig)| {
            let (s, token) = (s.clone(), token.clone());
            std::thread::spawn(move || register_with(&s, &token, &id, &sig).json())
        })
        .collect();
    let numbers: std::collections::HashSet<String> = threads
        .into_iter()
        .map(|t| t.join().unwrap()["number"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(numbers.len(), 1);
    assert_eq!(device_count(&s), 1);
}
