//! Moving a number to a replacement device: signed by the old key, or – if the device is lost –
//! with the account password, a cool-down and a notice to the remaining devices.

mod common;
use common::device::{next_json, runtime, Challenge, TestDevice};
use common::TestServer;
use serde_json::{json, Value};

const OLD: &str = "ultar-device-replace-old-v1";
const NEW: &str = "ultar-device-replace-new-v1";
const RECOVER: &str = "ultar-device-recover-v1";
const HOUR_MS: u64 = 3_600_000;

fn code(reply: &common::Reply) -> (u16, String) {
    (reply.status, reply.error_code())
}

fn works(s: &TestServer, token: &str) -> bool {
    s.get_as("/v1/account/entitlements", token).status == 200
}

/// The old device's signature for "replace me by `new`" over challenge `c`.
fn old_side(old: &TestDevice, new: &TestDevice, c: &Challenge) -> String {
    old.sign(OLD, c, &new.public())
}

fn replace_body(c: &Challenge, old: &TestDevice, old_sig: &str, new_sig: &str) -> Value {
    json!({"challenge_id": c.id, "old_device_id": old.id(), "old_signature": old_sig,
           "new_signature": new_sig, "name": "replacement"})
}

#[test]
fn replacement_signed_by_the_old_key_moves_the_number() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (old, new) = (TestDevice::new(1), TestDevice::new(2));
    let number = old.number(&s, &token);
    let old_token = old.session(&s);
    runtime().block_on(async {
        let (mut old_ws, _) = old.signal(&s).await;
        let c = new.challenge(&s, "replace", Some(&token));
        let body = replace_body(
            &c,
            &old,
            &old_side(&old, &new, &c),
            &new.sign(NEW, &c, &old.public()),
        );
        let reply = s.post("/v1/device/replace", Some(&token), &body);
        assert_eq!(reply.status, 200, "{}", reply.text());
        let v = reply.json();
        assert_eq!(v["number"], number.as_str(), "the number moved");
        assert_eq!(v["device_id"], new.id().as_str());
        assert!(works(&s, v["token"].as_str().unwrap()));
        // The old device is out: token dead, socket closed, key retired.
        assert!(!works(&s, &old_token));
        assert_eq!(next_json(&mut old_ws).await["error"], "device_removed");
        assert_eq!(code(&old.session_reply(&s)), (410, "device_retired".into()));
        assert_eq!(
            code(&old.register_reply(&s, &token, "")),
            (410, "device_retired".into())
        );
        // The new device answers the number.
        let (_ws, ready) = new.signal(&s).await;
        assert_eq!(ready["number"], number.as_str());
        // The same authorisation cannot be replayed.
        assert_eq!(
            code(&s.post("/v1/device/replace", Some(&token), &body)),
            (404, "device_unknown".into())
        );
    });
    let list = s.get_as("/v1/devices", &token).json();
    assert_eq!(list["devices"].as_array().unwrap().len(), 1);
    assert_eq!(list["devices"][0]["name"], "replacement");
}

/// Accounts and devices of the refused-replacement test.
struct Exchange {
    s: TestServer,
    anna: String,
    bert: String,
    old: TestDevice,
    new: TestDevice,
    other: TestDevice,
}

impl Exchange {
    fn attempt(&self, token: &str, c: &Challenge, old_sig: &str, new_sig: &str) -> (u16, String) {
        code(&self.s.post(
            "/v1/device/replace",
            Some(token),
            &replace_body(c, &self.old, old_sig, new_sig),
        ))
    }

    fn challenge(&self, token: &str) -> Challenge {
        self.new.challenge(&self.s, "replace", Some(token))
    }
}

#[test]
fn replacement_needs_both_signatures_over_this_exact_exchange() {
    let s = TestServer::start();
    let (anna, bert) = (s.register("anna"), s.register("bert"));
    let x = Exchange {
        s,
        anna,
        bert,
        old: TestDevice::new(1),
        new: TestDevice::new(2),
        other: TestDevice::new(3),
    };
    let number = x.old.number(&x.s, &x.anna);
    wrong_signatures_are_refused(&x);
    foreign_accounts_and_known_keys_are_refused(&x);
    // After all of that the old device still holds its number.
    assert_eq!(x.old.session_reply(&x.s).json()["number"], number.as_str());
}

fn wrong_signatures_are_refused(x: &Exchange) {
    let (anna, old, new, other) = (&x.anna, &x.old, &x.new, &x.other);
    let invalid = (401, "signature_invalid".to_string());

    // old signature made by another key (password thief without the old device)
    let c = x.challenge(anna);
    let forged = other.sign_as(OLD, &c.server, c.account_id, &c.bytes, &new.public());
    assert_eq!(
        x.attempt(anna, &c, &forged, &new.sign(NEW, &c, &old.public())),
        invalid
    );
    // old device authorised a DIFFERENT new key
    let c = x.challenge(anna);
    assert_eq!(
        x.attempt(
            anna,
            &c,
            &old_side(old, other, &c),
            &new.sign(NEW, &c, &old.public())
        ),
        invalid
    );
    // old device signed another challenge (an authorisation from an earlier attempt)
    let earlier = x.challenge(anna);
    let stale = old_side(old, new, &earlier);
    let c = x.challenge(anna);
    assert_eq!(
        x.attempt(anna, &c, &stale, &new.sign(NEW, &c, &old.public())),
        invalid
    );
    // new device does not prove its key
    let c = x.challenge(anna);
    let no_pop = other.sign_as(NEW, &c.server, c.account_id, &c.bytes, &old.public());
    assert_eq!(
        x.attempt(anna, &c, &old_side(old, new, &c), &no_pop),
        invalid
    );
    // the two signatures swapped
    x.s.clock.advance_millis(120_000);
    let c = x.challenge(anna);
    assert_eq!(
        x.attempt(
            anna,
            &c,
            &new.sign(NEW, &c, &old.public()),
            &old_side(old, new, &c)
        ),
        invalid
    );
}

fn foreign_accounts_and_known_keys_are_refused(x: &Exchange) {
    let (s, anna, bert) = (&x.s, &x.anna, &x.bert);
    let (old, new, other) = (&x.old, &x.new, &x.other);
    // another account cannot move anna's device, even with every signature in hand
    let c = x.challenge(anna);
    let (o, n) = (old_side(old, new, &c), new.sign(NEW, &c, &old.public()));
    assert_eq!(
        x.attempt(bert, &c, &o, &n),
        (404, "device_unknown".to_string())
    );
    let c = x.challenge(bert);
    let (o, n) = (old_side(old, new, &c), new.sign(NEW, &c, &old.public()));
    assert_eq!(
        x.attempt(bert, &c, &o, &n),
        (404, "device_unknown".to_string())
    );
    // a new key that is registered already cannot take a second number
    other.register(s, anna);
    s.clock.advance_millis(120_000);
    let c = other.challenge(s, "replace", Some(anna));
    let body = replace_body(
        &c,
        old,
        &old_side(old, other, &c),
        &other.sign(NEW, &c, &old.public()),
    );
    assert_eq!(
        code(&s.post("/v1/device/replace", Some(anna), &body)),
        (409, "device_taken".into())
    );
}

fn recover_start(
    s: &TestServer,
    token: &str,
    password: &str,
    lost: &str,
    new: &TestDevice,
) -> common::Reply {
    let c = new.challenge(s, "recover", Some(token));
    s.post(
        "/v1/device/recover/start",
        Some(token),
        &json!({"password": password, "old_device_id": lost, "challenge_id": c.id,
                "signature": new.sign(RECOVER, &c, lost.as_bytes())}),
    )
}

fn recover_finish(s: &TestServer, token: &str, lost: &str, new: &TestDevice) -> common::Reply {
    let c = new.challenge(s, "recover", Some(token));
    s.post(
        "/v1/device/recover/finish",
        Some(token),
        &json!({"old_device_id": lost, "challenge_id": c.id, "name": "new phone",
                "signature": new.sign(RECOVER, &c, lost.as_bytes())}),
    )
}

#[test]
fn a_lost_device_is_replaced_after_password_cool_down_and_notice() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (lost, tablet, new) = (TestDevice::new(1), TestDevice::new(2), TestDevice::new(3));
    let number = lost.number(&s, &token);
    tablet.register(&s, &token);
    let tablet_token = tablet.session(&s);
    runtime().block_on(async {
        let (mut tablet_ws, _) = tablet.signal(&s).await;
        // wrong password: nothing starts
        assert_eq!(
            code(&recover_start(
                &s,
                &token,
                "wrong-password",
                &lost.id(),
                &new
            )),
            (401, "invalid_credentials".into())
        );
        let started = recover_start(&s, &token, "password-123", &lost.id(), &new);
        assert_eq!(started.status, 200, "{}", started.text());
        assert_eq!(started.json()["eligible_in"], 72 * 3600);
        // the remaining device is told at once, and the notice is kept for devices that are offline
        let notice = next_json(&mut tablet_ws).await;
        assert_eq!(
            notice,
            json!({"type":"notice","event":"recovery_started","device": lost.id()[..8]})
        );
        let events = s.get_as("/v1/device/events", &tablet_token).json();
        assert_eq!(
            events["events"].as_array().unwrap().last().unwrap()["event"],
            "recovery_started"
        );
    });
    cool_down_then_finish(&s, &token, &lost, &new, &number);
}

/// Second half of the test above: the recovery is started, the clock has not moved yet.
fn cool_down_then_finish(
    s: &TestServer,
    token: &str,
    lost: &TestDevice,
    new: &TestDevice,
    number: &str,
) {
    // before the cool-down: refused, with the remaining time
    s.clock.advance_millis(71 * HOUR_MS);
    let early = recover_finish(s, token, &lost.id(), new);
    assert_eq!(code(&early), (409, "cooldown_active".into()));
    assert_eq!(early.header("retry-after"), Some("3600"));
    // asking to start again does not shorten or restart anything
    assert_eq!(
        recover_start(s, token, "password-123", &lost.id(), new).json()["eligible_in"],
        3600
    );
    // another key cannot finish what this key started
    s.clock.advance_millis(HOUR_MS);
    let other = TestDevice::new(4);
    assert_eq!(
        code(&recover_finish(s, token, &lost.id(), &other)),
        (404, "recovery_unknown".into())
    );
    // the lost device still works until the very end
    assert_eq!(lost.session_reply(s).status, 200);

    let done = recover_finish(s, token, &lost.id(), new);
    assert_eq!(done.status, 200, "{}", done.text());
    assert_eq!(done.json()["number"], number);
    assert_eq!(code(&lost.session_reply(s)), (410, "device_retired".into()));
    assert_eq!(
        code(&recover_finish(s, token, &lost.id(), new)),
        (404, "recovery_unknown".into())
    );
    let names: Vec<String> = s.get_as("/v1/devices", token).json()["devices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["test device", "new phone"]);
}

#[test]
fn a_recovery_can_be_stopped_from_any_device_and_by_a_password_change() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (phone, thief_device) = (TestDevice::new(1), TestDevice::new(9));
    let number = phone.number(&s, &token);
    let phone_token = phone.session(&s);
    // The thief knows the password; the phone is not lost at all.
    let stolen = s
        .post(
            "/v1/account/login",
            None,
            &json!({"username":"anna","password":"password-123"}),
        )
        .json()["token"]
        .as_str()
        .unwrap()
        .to_string();
    let thief = Thief {
        s,
        stolen,
        phone,
        thief_device,
    };
    a_device_cancels_the_recovery(&thief, &phone_token);
    a_password_change_and_expiry_void_the_recovery(&thief, &token, &number);
}

/// The thief of the test above: he knows the password; the phone is not lost at all.
struct Thief {
    s: TestServer,
    stolen: String,
    phone: TestDevice,
    thief_device: TestDevice,
}

fn a_device_cancels_the_recovery(t: &Thief, phone_token: &str) {
    let (s, stolen, phone, thief_device) = (&t.s, &t.stolen, &t.phone, &t.thief_device);
    assert_eq!(
        recover_start(s, stolen, "password-123", &phone.id(), thief_device).status,
        200
    );
    // a second recovery cannot be stacked on top
    let another = TestDevice::new(8);
    assert_eq!(
        code(&recover_start(
            s,
            stolen,
            "password-123",
            &phone.id(),
            &another
        )),
        (409, "recovery_pending".into())
    );
    // cancelling needs a device, not the password
    assert_eq!(
        code(&s.post("/v1/device/recover/cancel", Some(stolen), &json!({}))),
        (401, "auth_required".into())
    );
    let cancel = s.post("/v1/device/recover/cancel", Some(phone_token), &json!({}));
    assert_eq!(cancel.json()["cancelled"], true);
    assert_eq!(
        code(&s.post("/v1/device/recover/cancel", Some(phone_token), &json!({}))),
        (404, "recovery_unknown".into())
    );
    s.clock.advance_millis(80 * HOUR_MS);
    assert_eq!(
        code(&recover_finish(s, stolen, &phone.id(), thief_device)),
        (404, "recovery_unknown".into())
    );
}

fn a_password_change_and_expiry_void_the_recovery(t: &Thief, token: &str, number: &str) {
    let (s, stolen, phone, thief_device) = (&t.s, &t.stolen, &t.phone, &t.thief_device);
    // Second attempt; this time the owner changes the password during the cool-down.
    assert_eq!(
        recover_start(s, stolen, "password-123", &phone.id(), thief_device).status,
        200
    );
    let owner = s.post(
        "/v1/account/password",
        Some(token),
        &json!({"old_password":"password-123","new_password":"better-password-1"}),
    );
    let new_token = owner.json()["token"].as_str().unwrap().to_string();
    s.clock.advance_millis(80 * HOUR_MS);
    // the thief's token died with the password change: he cannot even ask for a challenge
    assert_eq!(
        code(&thief_device.challenge_reply(s, "recover", Some(stolen))),
        (401, "auth_required".into())
    );
    assert_eq!(
        code(&recover_finish(s, &new_token, &phone.id(), thief_device)),
        (404, "recovery_unknown".into())
    );
    assert_eq!(
        phone.session_reply(s).json()["number"],
        number,
        "the owner kept the number"
    );

    // An expired recovery (not finished within 7 days after the cool-down) is void.
    let late = TestDevice::new(7);
    assert_eq!(
        recover_start(s, &new_token, "better-password-1", &phone.id(), &late).status,
        200
    );
    s.clock.advance_millis((72 + 7 * 24) * HOUR_MS);
    assert_eq!(
        code(&recover_finish(s, &new_token, &phone.id(), &late)),
        (404, "recovery_unknown".into())
    );
}

#[test]
fn recovery_refuses_wrong_proofs_and_foreign_devices() {
    let s = TestServer::start();
    let (anna, bert) = (s.register("anna"), s.register("bert"));
    let (phone, new) = (TestDevice::new(1), TestDevice::new(2));
    phone.register(&s, &anna);
    // bert cannot start a recovery of anna's device with his own password
    assert_eq!(
        code(&recover_start(&s, &bert, "password-123", &phone.id(), &new)),
        (404, "device_unknown".into())
    );
    // the proof must name the device that is to be replaced
    let c = new.challenge(&s, "recover", Some(&anna));
    let r = s.post(
        "/v1/device/recover/start",
        Some(&anna),
        &json!({"password":"password-123","old_device_id": phone.id(), "challenge_id": c.id,
                "signature": new.sign(RECOVER, &c, b"another-device")}),
    );
    assert_eq!(code(&r), (401, "signature_invalid".into()));
    // a register proof is not a recover proof
    let c = new.challenge(&s, "register", Some(&anna));
    let r = s.post(
        "/v1/device/recover/start",
        Some(&anna),
        &json!({"password":"password-123","old_device_id": phone.id(), "challenge_id": c.id,
                "signature": new.sign(RECOVER, &c, phone.id().as_bytes())}),
    );
    assert_eq!(code(&r), (400, "challenge_mismatch".into()));
    // the device itself cannot be its own replacement
    assert_eq!(
        code(&recover_start(
            &s,
            &anna,
            "password-123",
            &phone.id(),
            &phone
        )),
        (409, "device_taken".into())
    );
    let n: i64 = s
        .state
        .db
        .lock()
        .query_row("SELECT COUNT(*) FROM device_recoveries", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}
