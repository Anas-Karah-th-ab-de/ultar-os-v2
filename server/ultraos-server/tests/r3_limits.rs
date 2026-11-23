//! Security review r3 (device identity): flooding, oversized and hostile input, enumeration and
//! error-code oracles, the signalling socket before authentication. All tests pass on the
//! reviewed code; what they pin down as "known, by design" is listed in REPORT.md.

mod common;
use common::device::{next_json, open_signal, runtime, send_json, TestDevice};
use common::TestServer;
use futures_util::SinkExt;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

fn code(reply: &common::Reply) -> (u16, String) {
    (reply.status, reply.error_code())
}

fn ask(s: &TestServer, token: &str, device: &TestDevice) -> common::Reply {
    device.challenge_reply(s, "register", Some(token))
}

#[test]
fn one_account_cannot_fill_the_challenge_store() {
    let s = TestServer::start();
    let (anna, bert) = (s.register("anna"), s.register("bert"));
    let keys: Vec<TestDevice> = (1..=40).map(TestDevice::new).collect();
    let mut granted = 0;
    for key in &keys {
        let reply = ask(&s, &anna, key);
        if reply.status == 200 {
            granted += 1;
        } else {
            assert_eq!(code(&reply), (429, "challenge_capacity".into()));
            let wait: u64 = reply.header("retry-after").unwrap().parse().unwrap();
            assert!((1..=120).contains(&wait), "{wait}");
        }
    }
    assert_eq!(granted, 8, "different keys do not multiply the budget");
    assert_eq!(s.state.devices.challenges.len(), 8);
    // Another account is not affected, and the first one recovers when the challenges expire.
    assert_eq!(ask(&s, &bert, &keys[0]).status, 200);
    s.clock.advance_millis(119_000);
    assert_eq!(ask(&s, &anna, &keys[0]).status, 429);
    s.clock.advance_millis(1_000);
    assert_eq!(ask(&s, &anna, &keys[0]).status, 200);
    assert_eq!(s.state.devices.challenges.len(), 1, "expired ones are gone");
}

#[test]
fn oversized_and_hostile_bodies_are_refused_before_anything_is_consumed() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(1);
    let big = json!({"purpose": "register", "public_key": device.public_b64(), "pad": "x".repeat(17_000)});
    assert_eq!(
        s.post("/v1/device/challenge", Some(&token), &big).status,
        413
    );
    let deep = format!("{}{}", "[".repeat(6000), "]".repeat(6000));
    let headers = [
        ("content-type", "application/json"),
        ("authorization", &format!("Bearer {token}") as &str),
    ];
    let reply = common::request("POST", &s.url("/v1/device/register"), &headers, Some(&deep));
    assert_eq!(code(&reply), (400, "invalid_request".into()));
    // A bad name is refused before the challenge is touched: the same proof still works after.
    let c = device.challenge(&s, "register", Some(&token));
    let sig = device.sign("ultar-device-register-v1", &c, b"");
    for name in [
        "n".repeat(65),
        "a\u{0}b".to_string(),
        "line\nbreak".to_string(),
    ] {
        let body = json!({"challenge_id": c.id, "signature": sig, "name": name});
        let reply = s.post("/v1/device/register", Some(&token), &body);
        assert_eq!(code(&reply), (400, "invalid_request".into()), "{name:?}");
    }
    let body = json!({"challenge_id": c.id, "signature": sig, "name": "ok"});
    assert_eq!(
        s.post("/v1/device/register", Some(&token), &body).status,
        200
    );
    // Over-long ids never reach the database lookups.
    let device_token = device.session(&s);
    let long = json!({"device_id": "a".repeat(15_000)});
    let reply = s.post("/v1/device/remove", Some(&device_token), &long);
    assert_eq!(code(&reply), (404, "device_unknown".into()));
}

#[test]
fn a_foreign_device_id_answers_like_one_that_does_not_exist() {
    let s = TestServer::start();
    let (anna, mallory) = (s.register("anna"), s.register("mallory"));
    let (victim, own, new) = (TestDevice::new(1), TestDevice::new(2), TestDevice::new(3));
    victim.register(&s, &anna);
    own.register(&s, &mallory);
    let own_token = own.session(&s);
    let ghost = TestDevice::new(99).id();
    let probe = |id: &str| -> Vec<(u16, String)> {
        let c = new.challenge(&s, "replace", Some(&mallory));
        let replace = json!({
            "challenge_id": c.id,
            "old_device_id": id,
            "old_signature": new.sign("ultar-device-replace-old-v1", &c, &new.public()),
            "new_signature": new.sign("ultar-device-replace-new-v1", &c, &victim.public()),
        });
        let named = json!({"device_id": id, "name": "x"});
        let old_form = json!({"device_id": id, "pubkey": ""});
        s.clock.advance_millis(121_000);
        vec![
            code(&s.post("/v1/device/replace", Some(&mallory), &replace)),
            code(&s.post("/v1/device/rename", Some(&own_token), &named)),
            code(&s.post("/v1/device/remove", Some(&own_token), &named)),
            code(&s.post("/v1/device/register", Some(&mallory), &old_form)),
        ]
    };
    assert_eq!(probe(&victim.id()), probe(&ghost));
    assert_eq!(victim.session_reply(&s).status, 200);
}

/// Pins down the one deliberate oracle of the HTTP interface: whoever knows a *public key* learns
/// without authentication whether it is registered, retired, and to which account number.
/// Device ids (hashes of keys) do not open it. Listed in REPORT.md as accepted, minor.
#[test]
fn the_session_challenge_tells_the_state_of_a_known_public_key_only() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (active, removed, unknown) = (TestDevice::new(1), TestDevice::new(2), TestDevice::new(3));
    active.register(&s, &token);
    removed.register(&s, &token);
    let body = json!({"device_id": removed.id()});
    let device_token = active.session(&s);
    assert_eq!(
        s.post("/v1/device/remove", Some(&device_token), &body)
            .status,
        200
    );
    let state = |d: &TestDevice| code(&d.challenge_reply(&s, "session", None));
    assert_eq!(state(&active).0, 200);
    assert_eq!(state(&removed), (410, "device_retired".into()));
    assert_eq!(state(&unknown), (404, "device_unknown".into()));
    // The id alone is not a key: it is refused as malformed input, whatever its state.
    for id in [active.id(), removed.id(), unknown.id()] {
        let body = json!({"purpose": "session", "public_key": id});
        let reply = s.post("/v1/device/challenge", None, &body);
        assert_eq!(code(&reply), (400, "invalid_request".into()));
    }
}

async fn first_answer(s: &TestServer, hello: &Value) -> Value {
    let mut ws = open_signal(s).await;
    send_json(&mut ws, hello).await;
    next_json(&mut ws).await
}

#[test]
fn the_socket_tells_nothing_about_ids_that_are_not_active_devices() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (active, removed) = (TestDevice::new(1), TestDevice::new(2));
    let number = active.number(&s, &token);
    removed.register(&s, &token);
    let body = json!({"device_id": removed.id()});
    let device_token = active.session(&s);
    assert_eq!(
        s.post("/v1/device/remove", Some(&device_token), &body)
            .status,
        200
    );
    runtime().block_on(async {
        let refused = json!({"type":"error","error":"auth"});
        for id in [
            removed.id(),
            TestDevice::new(3).id(),
            number.clone(),
            format!("legacy:{}", "a".repeat(16)),
            "a".repeat(60_000),
        ] {
            let hello = json!({"type":"hello","device_id": id});
            assert_eq!(first_answer(&s, &hello).await, refused);
        }
        // Nothing is relayed before the handshake is complete.
        let relay = json!({"to": number, "type": "offer", "data": "x"});
        assert_eq!(first_answer(&s, &relay).await, refused);
        assert!(!s.state.hub.is_online(&number));
    });
}

#[test]
fn the_sender_of_a_signalling_message_cannot_be_forged() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (caller, callee) = (TestDevice::new(1), TestDevice::new(2));
    let caller_number = caller.number(&s, &token);
    let callee_number = callee.number(&s, &token);
    runtime().block_on(async {
        let (mut a, _) = caller.signal(&s).await;
        let (mut b, _) = callee.signal(&s).await;
        let forged = json!({"to": callee_number, "from": "UOS-0000-0001", "type": "offer"});
        send_json(&mut a, &forged).await;
        assert_eq!(next_json(&mut b).await["from"], caller_number.as_str());
        // An oversized frame ends the socket instead of being relayed.
        let huge = json!({"to": callee_number, "type": "offer", "data": "x".repeat(70_000)});
        let _ = a.send(Message::Text(huge.to_string())).await;
        assert_eq!(next_json(&mut a).await["closed"], true);
    });
}

#[test]
fn a_silent_socket_is_dropped_after_the_login_timeout() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(1);
    device.register(&s, &token);
    runtime().block_on(async {
        // One socket never says hello, one stops after the challenge.
        let mut mute = open_signal(&s).await;
        let mut stalled = open_signal(&s).await;
        send_json(
            &mut stalled,
            &json!({"type":"hello","device_id": device.id()}),
        )
        .await;
        assert_eq!(next_json(&mut stalled).await["type"], "challenge");
        let begun = Instant::now();
        for ws in [&mut mute, &mut stalled] {
            let end = tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    let frame = next_json(ws).await;
                    if frame["closed"] == true {
                        break;
                    }
                    assert_eq!(frame["error"], "auth");
                }
            });
            end.await.expect("the server kept a silent socket open");
        }
        assert!(begun.elapsed() >= Duration::from_secs(9));
    });
}
