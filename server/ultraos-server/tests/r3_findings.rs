//! Security review r3 (device identity, finding S15): regression tests for the defects found.
//! Each test states the behaviour `docs/DEVICE-IDENTITY.md` promises and FAILS on the reviewed
//! code. Details, severity and proposed fixes: `REPORT.md` of the review (R3-1, R3-2, R3-4, R3-5, R3-6).

mod common;
use common::device::{next_json, runtime, send_json, Socket, TestDevice};
use common::TestServer;
use serde_json::{json, Value};
use std::time::Duration;
use ultraos_server::guard::class_for;
use ultraos_server::ratelimit::{BucketConfig, LimitClass};

const PASSWORD: &str = "password-123";

/// What a thief of the password has: an account token from a fresh login.
fn login(s: &TestServer, user: &str) -> String {
    let body = json!({"username": user, "password": PASSWORD});
    let reply = s.post("/v1/account/login", None, &body);
    assert_eq!(reply.status, 200, "{}", reply.text());
    reply.json()["token"].as_str().unwrap().to_string()
}

fn active_number(s: &TestServer, number: &str) -> bool {
    let conn = s.state.db.lock();
    conn.query_row(
        "SELECT COUNT(*) FROM numbers WHERE number=?1 AND state=1",
        [number],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        == 1
}

/// R3-2 (major). §5.5: "Changes need a device token, i.e. a registered device in hand: a
/// password thief cannot remove devices or retire numbers." He can: the password gives an account
/// token, the account token registers his own key, and that registration answers with a device
/// token that is as good as the owner's.
#[test]
fn r3_2_a_password_thief_cannot_remove_the_owners_device_or_retire_its_number() {
    let s = TestServer::start();
    let owner = s.register("anna");
    let phone = TestDevice::new(1);
    let number = phone.number(&s, &owner);

    let stolen = login(&s, "anna");
    let thief = TestDevice::new(66);
    let thief_token = thief.register(&s, &stolen)["token"]
        .as_str()
        .unwrap()
        .to_string();
    let body = json!({"device_id": phone.id()});
    let reply = s.post("/v1/device/remove", Some(&thief_token), &body);

    assert_ne!(
        reply.status, 200,
        "a device registered with nothing but the password removed the owner's device"
    );
    assert!(
        active_number(&s, &number),
        "the owner's number was retired for good by a password thief"
    );
    assert_eq!(phone.session_reply(&s).status, 200, "owner's device lives");
}

async fn quiet_or_closed(ws: &mut Socket, wait: Duration) -> Value {
    match tokio::time::timeout(wait, next_json(ws)).await {
        Ok(frame) => frame,
        Err(_) => json!({"silent": true}),
    }
}

/// R3-4 (major). §5.4: "Removing or replacing a device closes its socket at once"; threat A5: the
/// holder of a removed device must not keep using its number. The hub keeps one entry per number;
/// a second socket of the same key replaces the entry and the first socket stays open, unknown to
/// `kick`. After the removal it still places calls under the retired number.
#[test]
fn r3_4_every_socket_of_a_removed_device_is_closed() {
    let s = TestServer::start();
    let (anna, bert) = (s.register("anna"), s.register("bert"));
    let (phone, tablet, callee) = (TestDevice::new(1), TestDevice::new(2), TestDevice::new(3));
    let phone_number = phone.number(&s, &anna);
    tablet.register(&s, &anna);
    // Added with the fix of R3-2: a second device removes the first only once approved by it.
    phone.approve(&s, &tablet);
    let callee_number = callee.number(&s, &bert);
    let tablet_token = tablet.session(&s);
    runtime().block_on(async {
        let (mut first, ready) = phone.signal(&s).await;
        assert_eq!(ready["type"], "ready");
        let (mut second, ready) = phone.signal(&s).await;
        assert_eq!(ready["type"], "ready");
        let (mut callee_ws, _) = callee.signal(&s).await;

        let body = json!({"device_id": phone.id()});
        let removed = s.post("/v1/device/remove", Some(&tablet_token), &body);
        assert_eq!(removed.status, 200);
        assert_eq!(next_json(&mut second).await["error"], "device_removed");

        // The removed device still holds its first socket and rings another account with it.
        let offer = json!({"to": callee_number, "type": "offer", "data": "sdp"});
        send_json(&mut first, &offer).await;
        let heard = quiet_or_closed(&mut callee_ws, Duration::from_millis(1500)).await;
        assert_eq!(
            heard,
            json!({"silent": true}),
            "a removed device placed a call as {phone_number}"
        );
        let state = quiet_or_closed(&mut first, Duration::from_millis(1500)).await;
        assert_eq!(state["closed"], true, "the older socket was left open");
    });
}

/// R3-4, second face: after a *replacement* the number lives on with the new device, and the old
/// device's forgotten socket keeps speaking under it.
#[test]
fn r3_4_a_replaced_device_cannot_speak_as_the_number_it_gave_away() {
    let s = TestServer::start();
    let (anna, bert) = (s.register("anna"), s.register("bert"));
    let (old, new, callee) = (TestDevice::new(1), TestDevice::new(2), TestDevice::new(3));
    let number = old.number(&s, &anna);
    let callee_number = callee.number(&s, &bert);
    runtime().block_on(async {
        let (mut first, _) = old.signal(&s).await;
        let (_second, _) = old.signal(&s).await;
        let (mut callee_ws, _) = callee.signal(&s).await;
        let c = new.challenge(&s, "replace", Some(&anna));
        let body = json!({
            "challenge_id": c.id,
            "old_device_id": old.id(),
            "old_signature": old.sign("ultar-device-replace-old-v1", &c, &new.public()),
            "new_signature": new.sign("ultar-device-replace-new-v1", &c, &old.public()),
        });
        let moved = s.post("/v1/device/replace", Some(&anna), &body);
        assert_eq!(moved.json()["number"], number.as_str());
        let bye = json!({"to": callee_number, "type": "bye"});
        send_json(&mut first, &bye).await;
        let heard = quiet_or_closed(&mut callee_ws, Duration::from_millis(1500)).await;
        assert_eq!(
            heard,
            json!({"silent": true}),
            "the replaced key still signals as {number}"
        );
    });
}

/// Registers a throw-away device with the account token and lets it remove itself: two notices,
/// no lasting change. Something a password thief can do a hundred times per account.
fn add_and_drop(s: &TestServer, account_token: &str, seed: u8) {
    let device = TestDevice::new(seed);
    let token = device.register(s, account_token)["token"]
        .as_str()
        .unwrap()
        .to_string();
    let body = json!({"device_id": device.id()});
    let reply = s.post("/v1/device/remove", Some(&token), &body);
    assert_eq!(reply.status, 200, "{}", reply.text());
}

fn recovery_pending(s: &TestServer) -> bool {
    let conn = s.state.db.lock();
    conn.query_row("SELECT COUNT(*) FROM device_recoveries", [], |r| {
        r.get::<_, i64>(0)
    })
    .unwrap()
        == 1
}

/// R3-1 (major). §5.7/§5.8, threat A3: a number must not be taken over "without the owner's
/// devices being told". Only the newest 64 notices are kept; the thief who started the recovery
/// pushes `recovery_started` out of the list with notices he produces himself. A device that was
/// not connected at that moment never learns that its number is about to move.
#[test]
fn r3_1_the_notice_of_a_pending_recovery_cannot_be_flushed_out() {
    let s = TestServer::start();
    let owner = s.register("anna");
    let phone = TestDevice::new(1);
    phone.register(&s, &owner);

    let stolen = login(&s, "anna");
    let taker = TestDevice::new(66);
    let c = taker.challenge(&s, "recover", Some(&stolen));
    let start = json!({
        "password": PASSWORD,
        "old_device_id": phone.id(),
        "challenge_id": c.id,
        "signature": taker.sign("ultar-device-recover-v1", &c, phone.id().as_bytes()),
    });
    assert_eq!(
        s.post("/v1/device/recover/start", Some(&stolen), &start)
            .status,
        200
    );
    for round in 0..33u8 {
        if round % 7 == 0 {
            s.clock.advance_millis(121_000); // challenge budget of the account
        }
        add_and_drop(&s, &stolen, 100 + round);
    }
    assert!(recovery_pending(&s), "the recovery is still running");

    // The owner's phone comes online and reads its notices from the start.
    let phone_token = phone.session(&s);
    let events = s.get_as("/v1/device/events?after=0", &phone_token).json();
    let told = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["event"] == "recovery_started");
    assert!(
        told,
        "a recovery is pending but its notice is gone from the list"
    );
}

/// R3-5 (major). The password is checked by three endpoints, but only login is in the strict
/// limit class. Whoever holds any token of an account (threat A4: a stolen account or device
/// token) tests passwords twenty times faster than at the login, and the password is what turns
/// a short-lived token into a number take-over (§5.7) or a lock-out of the owner (§5.9).
#[test]
fn r3_5_every_password_check_is_throttled_like_the_login() {
    for path in ["/v1/account/password", "/v1/device/recover/start"] {
        assert_eq!(class_for(path), Some(LimitClass::Auth), "{path}");
    }
    let tight = BucketConfig {
        capacity: 3,
        refill_per_minute: 1,
    };
    let s = TestServer::start_with(|c| c.limits.auth = tight);
    let token = s.register("anna");
    let mut answers = Vec::new();
    for guess in 0..6 {
        let body =
            json!({"old_password": format!("guess-{guess}-x"), "new_password": "whatever-9"});
        answers.push(s.post("/v1/account/password", Some(&token), &body).status);
    }
    assert!(
        answers.contains(&429),
        "six password guesses with a login budget of three: {answers:?}"
    );
}

/// R3-6 (minor). §4: at most 8 *pending* challenges per key. A challenge that was presented keeps
/// its slot until it expires, so the ninth sign-in of an honest device within two minutes is
/// refused – and nine unauthenticated requests that merely name the key lock the device out of
/// device tokens (and with them out of `recover/cancel`) for as long as they are repeated.
#[test]
fn r3_6_used_challenges_do_not_lock_a_device_out_of_its_sessions() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(1);
    device.register(&s, &token);
    for round in 1..=9 {
        let reply = device.session_reply(&s);
        assert_eq!(
            reply.status,
            200,
            "sign-in {round} of an honest device: {}",
            reply.text()
        );
    }
}
