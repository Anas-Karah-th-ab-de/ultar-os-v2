//! Device-bound sessions: device tokens are issued only against a signed challenge and are bound
//! to the device key; removal and password change end tokens at once; the signalling socket is
//! opened with the device key, never with an account password.

mod common;
use common::device::{next_json, open_signal, runtime, send_json, Challenge, TestDevice};
use common::TestServer;
use serde_json::json;

const SESSION: &str = "ultar-device-session-v1";

fn code(reply: &common::Reply) -> (u16, String) {
    (reply.status, reply.error_code())
}

fn works(s: &TestServer, token: &str) -> bool {
    s.get_as("/v1/account/entitlements", token).status == 200
}

fn session_with(s: &TestServer, challenge_id: &str, signature: &str) -> common::Reply {
    s.post(
        "/v1/device/session",
        None,
        &json!({"challenge_id": challenge_id, "signature": signature}),
    )
}

#[test]
fn a_device_token_needs_a_fresh_signature_of_the_registered_key() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (device, other) = (TestDevice::new(1), TestDevice::new(2));
    let registered = device.register(&s, &token);
    other.register(&s, &token);

    let reply = device.session_reply(&s);
    assert_eq!(reply.status, 200);
    let v = reply.json();
    assert_eq!(v["device_id"], device.id().as_str());
    assert_eq!(v["number"], registered["number"]);
    assert_eq!(v["account_id"], 1);
    let device_token = v["token"].as_str().unwrap();
    assert!(works(&s, device_token));
    assert_eq!(s.get_as("/v1/turn-cred", device_token).status, 200);

    // The challenge is bound to the claimed key: another registered key cannot sign it …
    let c = device.challenge(&s, "session", None);
    let forged = other.sign_as(SESSION, &c.server, c.account_id, &c.bytes, b"");
    assert_eq!(
        code(&session_with(&s, &c.id, &forged)),
        (401, "signature_invalid".into())
    );
    // … a signature for another purpose does not count, and nothing can be replayed.
    let c = device.challenge(&s, "session", None);
    let wrong_purpose = device.sign("ultar-device-register-v1", &c, b"");
    assert_eq!(
        code(&session_with(&s, &c.id, &wrong_purpose)),
        (401, "signature_invalid".into())
    );
    let c = device.challenge(&s, "session", None);
    let sig = device.sign(SESSION, &c, b"");
    assert_eq!(session_with(&s, &c.id, &sig).status, 200);
    assert_eq!(
        code(&session_with(&s, &c.id, &sig)),
        (400, "challenge_used".into())
    );
    // A register challenge cannot be turned into a session.
    let c = device.challenge(&s, "register", Some(&token));
    let sig = device.sign(SESSION, &c, b"");
    assert_eq!(
        code(&session_with(&s, &c.id, &sig)),
        (400, "challenge_mismatch".into())
    );
    // An unregistered key gets no challenge and so no token.
    assert_eq!(
        code(&TestDevice::new(9).session_reply(&s)),
        (404, "device_unknown".into())
    );
}

#[test]
fn a_token_is_bound_to_its_device_key_and_cannot_be_used_for_another_key() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (phone, tablet) = (TestDevice::new(1), TestDevice::new(2));
    phone.register(&s, &token);
    tablet.register(&s, &token);
    phone.approve(&s, &tablet);
    let (phone_token, tablet_token) = (phone.session(&s), tablet.session(&s));

    // The server knows which key a token belongs to, whoever presents it.
    let list = s.get_as("/v1/devices", &phone_token).json();
    let current: Vec<&str> = list["devices"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["current"] == true)
        .map(|d| d["device_id"].as_str().unwrap())
        .collect();
    assert_eq!(current, vec![phone.id().as_str()]);

    // Re-labelling a token for another key breaks its signature.
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
    let (body, mac) = phone_token.split_once('.').unwrap();
    let text = String::from_utf8(B64.decode(body).unwrap()).unwrap();
    assert!(
        text.starts_with("d1:1:") && text.contains(&phone.id()),
        "{text}"
    );
    let relabelled = text.replace(&phone.id(), &tablet.id());
    assert!(!works(&s, &format!("{}.{mac}", B64.encode(relabelled))));
    // A device token is not an account token and the other way round.
    let c = phone.challenge(&s, "upgrade", Some(&token));
    let upgrade = s.post(
        "/v1/device/upgrade",
        Some(&phone_token),
        &json!({"legacy_device_id": "x", "challenge_id": c.id, "signature": phone.sign("ultar-device-upgrade-v1", &c, b"x")}),
    );
    assert_eq!(code(&upgrade), (401, "auth_required".into()));

    // The token lives and dies with ITS device: removing the phone kills the phone's token only.
    let r = s.post(
        "/v1/device/remove",
        Some(&tablet_token),
        &json!({"device_id": phone.id()}),
    );
    assert_eq!(r.status, 200);
    assert!(
        !works(&s, &phone_token),
        "token of the removed device is dead at once"
    );
    assert!(works(&s, &tablet_token));
    assert_eq!(
        code(&phone.session_reply(&s)),
        (410, "device_retired".into())
    );
}

#[test]
fn a_challenge_issued_before_removal_gives_no_token_after_it() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (phone, tablet) = (TestDevice::new(1), TestDevice::new(2));
    phone.register(&s, &token);
    tablet.register(&s, &token);
    phone.approve(&s, &tablet);
    let c = phone.challenge(&s, "session", None);
    let sig = phone.sign(SESSION, &c, b"");
    let tablet_token = tablet.session(&s);
    s.post(
        "/v1/device/remove",
        Some(&tablet_token),
        &json!({"device_id": phone.id()}),
    );
    assert_eq!(
        code(&session_with(&s, &c.id, &sig)),
        (404, "device_unknown".into())
    );
}

#[test]
fn password_change_ends_all_tokens_and_devices_stay_registered() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(1);
    let number = device.number(&s, &token);
    let device_token = device.session(&s);
    let second_login = s
        .post(
            "/v1/account/login",
            None,
            &json!({"username":"anna","password":"password-123"}),
        )
        .json()["token"]
        .as_str()
        .unwrap()
        .to_string();

    let change = |with: Option<&str>, old: &str, new: &str| {
        s.post(
            "/v1/account/password",
            with,
            &json!({"old_password": old, "new_password": new}),
        )
    };
    assert_eq!(
        code(&change(None, "password-123", "new-password-9")),
        (401, "auth_required".into())
    );
    assert_eq!(
        code(&change(Some(&token), "wrong-password", "new-password-9")),
        (401, "invalid_credentials".into())
    );
    assert_eq!(
        code(&change(Some(&token), "password-123", "short")),
        (400, "invalid_request".into())
    );
    assert!(works(&s, &token), "failed attempts change nothing");

    let changed = change(Some(&token), "password-123", "new-password-9");
    assert_eq!(changed.status, 200, "{}", changed.text());
    let new_token = changed.json()["token"].as_str().unwrap().to_string();
    for (what, dead) in [
        ("account token", &token),
        ("second login", &second_login),
        ("device token", &device_token),
    ] {
        assert!(
            !works(&s, dead),
            "{what} still works after the password change"
        );
    }
    assert!(works(&s, &new_token));
    after_the_password_change(&s, &device, &number);
}

/// Second half of the test above.
fn after_the_password_change(s: &TestServer, device: &TestDevice, number: &str) {
    // old password is gone, new one works and yields a token of the new epoch
    let login = |pw: &str| {
        s.post(
            "/v1/account/login",
            None,
            &json!({"username":"anna","password":pw}),
        )
    };
    assert_eq!(
        code(&login("password-123")),
        (401, "invalid_credentials".into())
    );
    assert!(works(
        s,
        login("new-password-9").json()["token"].as_str().unwrap()
    ));
    // the device proves its key again and carries on with the same number
    let again = device.session_reply(s).json();
    assert_eq!(again["number"], number);
    assert!(works(s, again["token"].as_str().unwrap()));
    let events = s
        .get_as("/v1/device/events", again["token"].as_str().unwrap())
        .json();
    let kinds: Vec<&str> = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["event"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["device_added", "password_changed"]);
}

#[test]
fn device_tokens_expire_after_a_day() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(1);
    device.register(&s, &token);
    let device_token = device.session(&s);
    s.clock.advance_millis(86_400_000);
    assert!(works(&s, &device_token));
    s.clock.advance_millis(1_000);
    assert!(!works(&s, &device_token));
    assert!(works(&s, &device.session(&s)));
}

#[test]
fn events_need_a_device_token_and_page_by_id() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(1);
    let device_token = device.register(&s, &token)["token"]
        .as_str()
        .unwrap()
        .to_string();
    TestDevice::new(2).register(&s, &token);
    assert_eq!(
        code(&s.get_as("/v1/device/events", &token)),
        (401, "auth_required".into())
    );
    assert_eq!(
        code(&s.get("/v1/device/events")),
        (401, "auth_required".into())
    );
    let all = s.get_as("/v1/device/events", &device_token).json();
    let list = all["events"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[1]["device"], TestDevice::new(2).id()[..8]);
    let after = list[0]["id"].as_i64().unwrap();
    let rest = s
        .get_as(&format!("/v1/device/events?after={after}"), &device_token)
        .json();
    assert_eq!(rest["events"].as_array().unwrap().len(), 1);
    assert_eq!(
        code(&s.get_as("/v1/device/events?after=x", &device_token)),
        (400, "invalid_request".into())
    );
}

#[test]
fn signalling_is_opened_by_the_device_key_not_by_the_account() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (device, stranger) = (TestDevice::new(1), TestDevice::new(2));
    let number = device.number(&s, &token);
    runtime().block_on(async {
        // A thief with the password has an account token and knows the number: no socket.
        let login = s.post(
            "/v1/account/login",
            None,
            &json!({"username":"anna","password":"password-123"}),
        );
        let stolen = login.json()["token"].as_str().unwrap().to_string();
        for first in [
            json!({"token": stolen, "number": number}),
            json!({"type": "hello", "device_id": device.id(), "token": stolen}),
        ] {
            let mut ws = open_signal(&s).await;
            send_json(&mut ws, &first).await;
            assert_eq!(
                next_json(&mut ws).await,
                json!({"type":"error","error":"device_key_required"})
            );
            assert_eq!(next_json(&mut ws).await["closed"], true);
        }
        // He registers his own device: it gets its own number, never the victim's.
        let own = stranger.number(&s, &stolen);
        assert_ne!(own, number);
        let (_ws, ready) = stranger.signal(&s).await;
        assert_eq!(
            ready,
            json!({"type":"ready","number": own,"manage": false,"recovery": null}),
            "his device is an added one: it does not manage the account's devices"
        );
        // Claiming the victim's device id with his own key fails.
        let (_ws, refused) = device.signal_signed_by(&s, &stranger).await;
        assert_eq!(refused, json!({"type":"error","error":"auth"}));
        bad_handshakes_are_refused(&s, &device).await;
        // The real device connects.
        let (_ws, ready) = device.signal(&s).await;
        assert_eq!(
            ready,
            json!({"type":"ready","number": number,"manage": true,"recovery": null})
        );
    });
}

/// Part of the test above: nothing but the key's own signature over this socket's challenge opens it.
async fn bad_handshakes_are_refused(s: &TestServer, device: &TestDevice) {
    {
        // Unknown ids, malformed frames, a signature for another purpose: all "auth".
        for hello in [
            json!({"type":"hello","device_id": TestDevice::new(9).id()}),
            json!({"type":"hello","device_id": "UOS-0000-0000"}),
            json!({"type":"hi","device_id": device.id()}),
            json!({"device_id": 5}),
            json!([1, 2]),
        ] {
            let mut ws = open_signal(s).await;
            send_json(&mut ws, &hello).await;
            assert_eq!(next_json(&mut ws).await["error"], "auth", "{hello}");
        }
        let mut ws = open_signal(s).await;
        send_json(&mut ws, &json!({"type":"hello","device_id": device.id()})).await;
        let c = next_json(&mut ws).await;
        let challenge = Challenge::from_json(&c);
        let sig = device.sign("ultar-device-session-v1", &challenge, b"");
        send_json(&mut ws, &json!({"type":"auth","signature": sig})).await;
        assert_eq!(next_json(&mut ws).await["error"], "auth");
        // Each socket gets its own challenge.
        let mut ws2 = open_signal(s).await;
        send_json(&mut ws2, &json!({"type":"hello","device_id": device.id()})).await;
        assert_ne!(next_json(&mut ws2).await["challenge"], c["challenge"]);
    }
}

#[test]
fn removing_a_device_closes_its_socket_and_others_are_told() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (phone, tablet) = (TestDevice::new(1), TestDevice::new(2));
    let phone_number = phone.number(&s, &token);
    tablet.register(&s, &token);
    phone.approve(&s, &tablet);
    let tablet_token = tablet.session(&s);
    runtime().block_on(async {
        let (mut phone_ws, _) = phone.signal(&s).await;
        let (mut tablet_ws, _) = tablet.signal(&s).await;
        assert!(s.state.hub.is_online(&phone_number));
        let r = s.post(
            "/v1/device/remove",
            Some(&tablet_token),
            &json!({"device_id": phone.id()}),
        );
        assert_eq!(r.status, 200);
        assert!(
            !s.state.hub.is_online(&phone_number),
            "offline the moment it is removed"
        );
        assert_eq!(
            next_json(&mut phone_ws).await,
            json!({"type":"error","error":"device_removed"})
        );
        assert_eq!(next_json(&mut phone_ws).await["closed"], true);
        let notice = next_json(&mut tablet_ws).await;
        assert_eq!(
            notice,
            json!({"type":"notice","event":"device_removed","device": phone.id()[..8]})
        );
        // The removed device cannot come back to the socket.
        let (_ws, refused) = phone.signal(&s).await;
        assert_eq!(refused["error"], "auth");
        // Calls to the retired number are unreachable.
        send_json(
            &mut tablet_ws,
            &json!({"to": phone_number, "type": "offer"}),
        )
        .await;
        assert_eq!(next_json(&mut tablet_ws).await["type"], "unreachable");
    });
}
