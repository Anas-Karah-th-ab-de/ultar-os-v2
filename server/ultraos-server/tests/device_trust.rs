//! Review round 3: what a password alone can and cannot do. An added device manages the account's
//! other devices only after approval by one of them or after the cool-down (R3-2); a pending
//! recovery is reported as a state to every device (R3-1); password checks share one budget per
//! account (R3-5); the process limits its connections (R3-8).

mod common;
use common::device::{next_json, runtime, send_json, TestDevice};
use common::TestServer;
use serde_json::{json, Value};

const PASSWORD: &str = "password-123";
const COOLDOWN_MS: u64 = 72 * 3_600_000;

fn code(reply: &common::Reply) -> (u16, String) {
    (reply.status, reply.error_code())
}

fn login(s: &TestServer, user: &str, password: &str) -> common::Reply {
    let body = json!({"username": user, "password": password});
    s.post("/v1/account/login", None, &body)
}

/// Status of the four management calls with `token`, aimed at `target`.
fn management(s: &TestServer, token: &str, target: &str) -> Vec<(u16, String)> {
    let rename = json!({"device_id": target, "name": "x"});
    let named = json!({"device_id": target});
    vec![
        code(&s.post("/v1/device/rename", Some(token), &rename)),
        code(&s.get_as("/v1/device/events", token)),
        code(&s.post("/v1/device/recover/cancel", Some(token), &json!({}))),
        code(&s.post("/v1/device/remove", Some(token), &named)),
    ]
}

#[test]
fn a_device_added_with_the_password_manages_nothing_until_approved_or_waited() {
    let s = TestServer::start();
    let owner = s.register("anna");
    let (phone, added) = (TestDevice::new(1), TestDevice::new(2));
    let first = phone.register(&s, &owner);
    assert_eq!(
        (first["manage"].as_bool(), first["manage_in"].as_i64()),
        (Some(true), Some(0))
    );
    let stolen = login(&s, "anna", PASSWORD).json()["token"]
        .as_str()
        .unwrap()
        .to_string();
    let second = added.register(&s, &stolen);
    assert_eq!(second["manage"], false);
    assert_eq!(second["manage_in"], 72 * 3600);
    let token = second["token"].as_str().unwrap().to_string();

    let refused = (403, "device_not_approved".to_string());
    assert_eq!(
        management(&s, &token, &phone.id()),
        vec![refused.clone(); 4]
    );
    let reply = s.post(
        "/v1/device/remove",
        Some(&token),
        &json!({"device_id": phone.id()}),
    );
    assert_eq!(reply.header("retry-after"), Some("259200"));
    // It cannot approve itself or anything else, with or without a signature of its own.
    assert_eq!(code(&added.approve_reply(&s, &added)), refused);
    assert_eq!(code(&added.approve_reply(&s, &phone)), refused);
    // The list tells who manages; the owner's device is untouched.
    let list = s.get_as("/v1/devices", &owner).json();
    let manage: Vec<bool> = list["devices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["manage"] == true)
        .collect();
    assert_eq!(manage, [true, false]);
    assert_eq!(list["devices"][1]["manage_in"], 72 * 3600);
    assert_eq!(
        list["devices"][1]["public_key"],
        added.public_b64().as_str()
    );
    assert_eq!(phone.session_reply(&s).json()["manage"], true);

    after_the_cool_down_it_manages(&s, &phone, &added);
}

/// Second half of the test above.
fn after_the_cool_down_it_manages(s: &TestServer, phone: &TestDevice, added: &TestDevice) {
    let refused = (403, "device_not_approved".to_string());
    // One second before the cool-down: still nothing. After it: a managing device.
    s.clock.advance_millis(COOLDOWN_MS - 1000);
    let token = added.session(s);
    assert_eq!(management(s, &token, &phone.id())[0], refused);
    s.clock.advance_millis(1000);
    let session = added.session_reply(s).json();
    assert_eq!(
        (session["manage"].as_bool(), session["manage_in"].as_i64()),
        (Some(true), Some(0))
    );
    let token = session["token"].as_str().unwrap();
    assert_eq!(
        s.post(
            "/v1/device/rename",
            Some(token),
            &json!({"device_id": phone.id(), "name": "x"})
        )
        .status,
        200
    );
}

#[test]
fn an_added_device_keeps_its_own_number_and_may_remove_itself() {
    let s = TestServer::start();
    let owner = s.register("anna");
    let (phone, added) = (TestDevice::new(1), TestDevice::new(2));
    phone.register(&s, &owner);
    let second = added.register(&s, &owner);
    let token = second["token"].as_str().unwrap();
    runtime().block_on(async {
        let (_ws, ready) = added.signal(&s).await;
        assert_eq!(ready["number"], second["number"]);
        assert_eq!(ready["manage"], false);
    });
    assert_eq!(s.get_as("/v1/devices", token).status, 200);
    assert_eq!(s.get_as("/v1/turn-cred", token).status, 200);
    let body = json!({"device_id": added.id()});
    assert_eq!(
        s.post("/v1/device/remove", Some(token), &body).json()["removed"],
        true
    );
    assert_eq!(
        s.get_as("/v1/devices", &owner).json()["devices"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

fn approval(
    s: &TestServer,
    token: &str,
    signer: &TestDevice,
    purpose: &str,
    target: &TestDevice,
    extra: &[u8],
) -> common::Reply {
    let c = signer.challenge(s, purpose, Some(token));
    let signature = signer.sign("ultar-device-approve-v1", &c, extra);
    let body = json!({"device_id": target.id(), "challenge_id": c.id, "signature": signature});
    s.post("/v1/device/approve", Some(token), &body)
}

/// One refused approval: signer, challenge purpose, target, signed extra data, expected answer.
type Case<'a> = (
    &'a TestDevice,
    &'a str,
    &'a TestDevice,
    Vec<u8>,
    (u16, &'a str),
);

/// Everything that must not approve `added` (devices: phone, added, other, foreign).
fn refused_approvals(s: &TestServer, anna: &str, devices: [&TestDevice; 4]) {
    let [phone, added, other, foreign] = devices;
    let phone_token = phone.session(s);
    // the account token (password) approves nothing
    assert_eq!(
        code(&approval(s, anna, phone, "approve", added, &added.public())),
        (401, "auth_required".into())
    );
    let ghost = TestDevice::new(9);
    let (invalid, mismatch, unknown) = (
        (401, "signature_invalid"),
        (400, "challenge_mismatch"),
        (404, "device_unknown"),
    );
    // (signer, challenge purpose, target, signed extra data, expected answer)
    let cases: [Case<'_>; 6] = [
        // signature over another key, or over nothing
        (phone, "approve", added, other.public().to_vec(), invalid),
        (phone, "approve", added, Vec::new(), invalid),
        // made by another device than the token's
        (added, "approve", added, added.public().to_vec(), mismatch),
        // a challenge of another purpose
        (phone, "register", added, added.public().to_vec(), mismatch),
        // a device of another account; an unknown device
        (
            phone,
            "approve",
            foreign,
            foreign.public().to_vec(),
            unknown,
        ),
        (phone, "approve", &ghost, vec![0; 32], unknown),
    ];
    for (signer, purpose, target, extra, expected) in cases {
        let reply = approval(s, &phone_token, signer, purpose, target, &extra);
        assert_eq!(
            code(&reply),
            (expected.0, expected.1.to_string()),
            "{purpose} for {}",
            target.id()
        );
    }
}

#[test]
fn approval_needs_the_managing_devices_token_and_its_signature_over_the_new_key() {
    let s = TestServer::start();
    let (anna, bert) = (s.register("anna"), s.register("bert"));
    let (phone, added, other, foreign) = (
        TestDevice::new(1),
        TestDevice::new(2),
        TestDevice::new(3),
        TestDevice::new(4),
    );
    phone.register(&s, &anna);
    added.register(&s, &anna);
    other.register(&s, &anna);
    foreign.register(&s, &bert);
    refused_approvals(&s, &anna, [&phone, &added, &other, &foreign]);
    let unapproved = added.session_reply(&s).json();
    assert_eq!(
        unapproved["manage"], false,
        "nothing so far approved anything"
    );

    let phone_token = phone.session(&s);
    let ok = approval(&s, &phone_token, &phone, "approve", &added, &added.public());
    assert_eq!(ok.json()["approved"], true);
    let now = added.session_reply(&s).json();
    assert_eq!(now["manage"], true);
    // the approved device is a full manager: it approves the third and reads the notices
    added.approve(&s, &other);
    let events = s
        .get_as("/v1/device/events", now["token"].as_str().unwrap())
        .json();
    let kinds: Vec<&str> = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["event"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "device_added",
            "device_added",
            "device_added",
            "device_approved",
            "device_approved"
        ]
    );
}

#[test]
fn a_pending_recovery_is_reported_to_every_device_until_it_is_over() {
    let s = TestServer::start();
    let owner = s.register("anna");
    let (phone, taker) = (TestDevice::new(1), TestDevice::new(66));
    phone.register(&s, &owner);
    assert_eq!(phone.session_reply(&s).json()["recovery"], Value::Null);
    let c = taker.challenge(&s, "recover", Some(&owner));
    let start = json!({"password": PASSWORD, "old_device_id": phone.id(), "challenge_id": c.id,
        "signature": taker.sign("ultar-device-recover-v1", &c, phone.id().as_bytes())});
    assert_eq!(
        s.post("/v1/device/recover/start", Some(&owner), &start)
            .status,
        200
    );
    s.clock.advance_millis(3_600_000);
    let expected = json!({"old_device": phone.id(), "eligible_in": 71 * 3600});
    // The owner's phone was offline when it started: every answer about itself tells it.
    let session = phone.session_reply(&s).json();
    assert_eq!(session["recovery"], expected);
    assert_eq!(s.get_as("/v1/devices", &owner).json()["recovery"], expected);
    runtime().block_on(async {
        let (_ws, ready) = phone.signal(&s).await;
        assert_eq!(ready["recovery"], expected);
    });
    let token = session["token"].as_str().unwrap();
    assert_eq!(
        s.post("/v1/device/recover/cancel", Some(token), &json!({}))
            .status,
        200
    );
    assert_eq!(phone.session_reply(&s).json()["recovery"], Value::Null);
}

#[test]
fn notices_are_pushed_to_managing_devices_only_and_a_new_sign_in_replaces_the_socket() {
    let s = TestServer::start();
    let owner = s.register("anna");
    let (phone, added) = (TestDevice::new(1), TestDevice::new(2));
    let number = phone.number(&s, &owner);
    added.register(&s, &owner);
    runtime().block_on(async {
        let (mut first, _) = phone.signal(&s).await;
        let (mut added_ws, _) = added.signal(&s).await;
        // The phone signs in again (new network): its previous socket is closed, silently.
        let (mut second, ready) = phone.signal(&s).await;
        assert_eq!(ready["number"], number.as_str());
        assert_eq!(next_json(&mut first).await, json!({"closed": true}));
        assert!(s.state.hub.is_online(&number));
        // A third device is registered: the managing phone is told, the added device is not.
        TestDevice::new(3).register(&s, &owner);
        assert_eq!(next_json(&mut second).await["event"], "device_added");
        send_json(
            &mut added_ws,
            &json!({"to": "UOS-0000-0000", "type": "offer"}),
        )
        .await;
        assert_eq!(
            next_json(&mut added_ws).await["type"],
            "unreachable",
            "no notice came before it"
        );
    });
}

/// From one client: login, password change and recovery start share the client's budget for
/// the account (`password_failures`, here 4).
#[test]
fn failed_password_checks_of_one_client_share_one_budget() {
    let s = TestServer::start_with(|c| c.devices.password_failures = 4);
    let token = s.register("anna");
    let other = s.register("bert");
    let device = TestDevice::new(1);
    device.register(&s, &token);
    let change = |old: &str| json!({"old_password": old, "new_password": "another-password-1"});
    // two wrong logins, one wrong password change, one wrong recovery start: four failures
    assert_eq!(login(&s, "anna", "wrong-1-x").status, 401);
    assert_eq!(login(&s, "anna", "wrong-2-x").status, 401);
    assert_eq!(
        s.post("/v1/account/password", Some(&token), &change("wrong-3-x"))
            .status,
        401
    );
    let taker = TestDevice::new(2);
    let c = taker.challenge(&s, "recover", Some(&token));
    let start = json!({"password": "wrong-4-x", "old_device_id": device.id(), "challenge_id": c.id,
        "signature": taker.sign("ultar-device-recover-v1", &c, device.id().as_bytes())});
    assert_eq!(
        s.post("/v1/device/recover/start", Some(&token), &start)
            .status,
        401
    );
    // now even the right password is refused from this client, on every path, with the wait
    let locked = login(&s, "anna", PASSWORD);
    assert_eq!(code(&locked), (429, "rate_limited".into()));
    assert_eq!(locked.header("retry-after"), Some("900"));
    assert_eq!(
        s.post("/v1/account/password", Some(&token), &change(PASSWORD))
            .status,
        429
    );
    // other accounts and the account's devices are not affected
    assert_eq!(login(&s, "bert", PASSWORD).status, 200);
    assert_eq!(s.get_as("/v1/devices", &other).status, 200);
    assert_eq!(device.session_reply(&s).status, 200);
    // after the window the right password works, and right passwords never use the budget up
    s.clock.advance_millis(15 * 60 * 1000);
    for _ in 0..10 {
        assert_eq!(login(&s, "anna", PASSWORD).status, 200);
    }
}

/// Opens a connection that is busy: its request head is complete, its body never comes.
fn busy_connection(s: &TestServer) -> std::net::TcpStream {
    use std::io::Write;
    let mut stream = std::net::TcpStream::connect(s.addr).unwrap();
    let head = "POST /v1/account/login HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 400\r\n\r\n{";
    stream.write_all(head.as_bytes()).unwrap();
    stream
}

/// Sends a health request on a new connection; true when it is answered.
fn served(s: &TestServer) -> bool {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(s.addr).unwrap();
    let _ = stream.write_all(b"GET /v1/health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let mut buffer = [0u8; 64];
    let n = stream.read(&mut buffer).unwrap_or(0);
    String::from_utf8_lossy(&buffer[..n]).starts_with("HTTP/1.1 200")
}

#[test]
fn the_number_of_open_connections_is_bounded_and_silent_ones_give_way() {
    let s = TestServer::start_with(|c| c.max_connections = 2);
    // Silent connections take no place from a client with a request: the oldest gives way.
    let silent: Vec<std::net::TcpStream> = (0..2)
        .map(|_| std::net::TcpStream::connect(s.addr).unwrap())
        .collect();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(served(&s), "a silent connection kept a request out");
    drop(silent);
    // Two connections in the middle of a request are not closed for a newcomer.
    let busy = [busy_connection(&s), busy_connection(&s)];
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!served(&s), "served beyond the connection cap");
    drop(busy);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(served(&s));
}

#[test]
fn one_client_gets_only_its_share_of_the_connections() {
    // Without trusted proxies the loopback address is an ordinary client.
    let s = TestServer::start_with(|c| {
        c.max_connections_per_peer = 2;
        c.trusted_proxies = Vec::new();
    });
    let busy = [busy_connection(&s), busy_connection(&s)];
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!served(&s), "a third connection of the same client");
    drop(busy);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(served(&s));
}
