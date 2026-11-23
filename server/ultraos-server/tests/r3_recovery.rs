//! Security review r3 (device identity): abuse of the lost-device recovery (cool-down bypass,
//! cancelling from outside, finishing with another key) and use of tokens after the device or the
//! password changed. Every test is an attack that must fail.

mod common;
use common::device::TestDevice;
use common::TestServer;
use serde_json::{json, Value};

const RECOVER: &str = "ultar-device-recover-v1";
// Adapted after review round 3: the end of a recovery has a label of its own.
const FINISH: &str = "ultar-device-recover-finish-v1";
const PASSWORD: &str = "password-123";
const COOLDOWN: i64 = 72 * 3600;

fn code(reply: &common::Reply) -> (u16, String) {
    (reply.status, reply.error_code())
}

fn start(s: &TestServer, token: &str, lost: &str, new: &TestDevice, pw: &str) -> common::Reply {
    let c = new.challenge(s, "recover", Some(token));
    let body = json!({
        "password": pw,
        "old_device_id": lost,
        "challenge_id": c.id,
        "signature": new.sign(RECOVER, &c, lost.as_bytes()),
    });
    s.post("/v1/device/recover/start", Some(token), &body)
}

fn finish(s: &TestServer, token: &str, lost: &str, new: &TestDevice) -> common::Reply {
    let c = new.challenge(s, "recover", Some(token));
    let body = json!({
        "old_device_id": lost,
        "challenge_id": c.id,
        "signature": new.sign(FINISH, &c, lost.as_bytes()),
        "name": "new phone",
    });
    s.post("/v1/device/recover/finish", Some(token), &body)
}

fn pending(s: &TestServer) -> i64 {
    let conn = s.state.db.lock();
    conn.query_row("SELECT COUNT(*) FROM device_recoveries", [], |r| r.get(0))
        .unwrap()
}

fn step(s: &TestServer, secs: i64) {
    s.clock.advance_millis(u64::try_from(secs).unwrap() * 1000);
}

#[test]
fn the_cool_down_cannot_be_skipped_shortened_or_restarted() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (lost, new, other) = (TestDevice::new(1), TestDevice::new(2), TestDevice::new(3));
    let number = lost.number(&s, &token);
    let id = lost.id();
    // A wrong password starts nothing, however good the proof.
    assert_eq!(
        code(&start(&s, &token, &id, &new, "password-124")),
        (401, "invalid_credentials".into())
    );
    assert_eq!(pending(&s), 0);
    let started = start(&s, &token, &id, &new, PASSWORD);
    assert_eq!(started.json()["eligible_in"], COOLDOWN);
    // At once, and one second before the end: refused, with the exact waiting time.
    let early = finish(&s, &token, &id, &new);
    assert_eq!(code(&early), (409, "cooldown_active".into()));
    assert_eq!(early.header("retry-after"), Some("259200"));
    step(&s, COOLDOWN - 1);
    let early = finish(&s, &token, &id, &new);
    assert_eq!(code(&early), (409, "cooldown_active".into()));
    assert_eq!(early.header("retry-after"), Some("1"));
    // Starting again neither restarts nor shortens the clock; another key cannot cut in.
    assert_eq!(
        start(&s, &token, &id, &new, PASSWORD).json()["eligible_in"],
        1
    );
    assert_eq!(
        code(&start(&s, &token, &id, &other, PASSWORD)),
        (409, "recovery_pending".into())
    );
    step(&s, 1);
    // Eligible now – but only for the key the recovery was started with.
    assert_eq!(
        code(&finish(&s, &token, &id, &other)),
        (404, "recovery_unknown".into())
    );
    assert_eq!(pending(&s), 1, "a foreign key's attempt does not cancel it");
    let done = finish(&s, &token, &id, &new);
    assert_eq!(done.status, 200, "{}", done.text());
    assert_eq!(done.json()["number"], number.as_str());
    assert_eq!(lost.session_reply(&s).status, 410);
}

#[test]
fn a_recovery_expires_and_cannot_be_finished_for_another_device() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (lost, second, new) = (TestDevice::new(4), TestDevice::new(5), TestDevice::new(6));
    lost.register(&s, &token);
    let second_number = second.number(&s, &token);
    assert_eq!(start(&s, &token, &lost.id(), &new, PASSWORD).status, 200);
    step(&s, COOLDOWN);
    // The recovery names `lost`; it does not move the number of another device of the account.
    assert_eq!(
        code(&finish(&s, &token, &second.id(), &new)),
        (404, "recovery_unknown".into())
    );
    assert_eq!(
        second.session_reply(&s).json()["number"],
        second_number.as_str()
    );
    // Seven days after eligibility it is gone.
    step(&s, 7 * 86_400);
    assert_eq!(
        code(&finish(&s, &token, &lost.id(), &new)),
        (404, "recovery_unknown".into())
    );
    assert_eq!(pending(&s), 0);
    assert_eq!(lost.session_reply(&s).status, 200, "nothing moved");
}

#[test]
fn only_a_device_of_the_account_cancels_a_recovery() {
    let s = TestServer::start();
    let (anna, mallory) = (s.register("anna"), s.register("mallory"));
    let (lost, new, foreign) = (TestDevice::new(7), TestDevice::new(8), TestDevice::new(9));
    lost.register(&s, &anna);
    foreign.register(&s, &mallory);
    assert_eq!(start(&s, &anna, &lost.id(), &new, PASSWORD).status, 200);
    let cancel =
        |token: Option<&str>| code(&s.post("/v1/device/recover/cancel", token, &json!({})));
    assert_eq!(cancel(None), (401, "auth_required".into()));
    assert_eq!(cancel(Some(&anna)), (401, "auth_required".into()));
    assert_eq!(cancel(Some(&mallory)), (401, "auth_required".into()));
    let foreign_token = foreign.session(&s);
    assert_eq!(
        cancel(Some(&foreign_token)),
        (404, "recovery_unknown".into())
    );
    assert_eq!(pending(&s), 1, "another account's device cancelled nothing");
    // The device reported lost cancels it, and its notice is on record.
    let lost_token = lost.session(&s);
    assert_eq!(cancel(Some(&lost_token)).0, 200);
    assert_eq!(pending(&s), 0);
    step(&s, COOLDOWN);
    assert_eq!(
        code(&finish(&s, &anna, &lost.id(), &new)),
        (404, "recovery_unknown".into())
    );
}

#[test]
fn recovery_does_not_reach_into_another_account() {
    let s = TestServer::start();
    let (anna, mallory) = (s.register("anna"), s.register("mallory"));
    let (victim, new) = (TestDevice::new(10), TestDevice::new(11));
    let number = victim.number(&s, &anna);
    // Mallory knows her own password and the victim's device id.
    let reply = start(&s, &mallory, &victim.id(), &new, PASSWORD);
    assert_eq!(code(&reply), (404, "device_unknown".into()));
    // The answer is the same as for an id that does not exist at all (no oracle).
    let ghost = TestDevice::new(12).id();
    let reply = start(&s, &mallory, &ghost, &new, PASSWORD);
    assert_eq!(code(&reply), (404, "device_unknown".into()));
    assert_eq!(pending(&s), 0);
    assert_eq!(victim.session_reply(&s).json()["number"], number.as_str());
}

fn works(s: &TestServer, token: &str) -> bool {
    s.get_as("/v1/devices", token).status == 200
}

fn device_calls(s: &TestServer, token: &str, target: &str) -> Vec<u16> {
    let named = json!({"device_id": target});
    let rename = json!({"device_id": target, "name": "x"});
    vec![
        s.post("/v1/device/remove", Some(token), &named).status,
        s.post("/v1/device/rename", Some(token), &rename).status,
        s.post("/v1/device/recover/cancel", Some(token), &json!({}))
            .status,
        s.get_as("/v1/device/events", token).status,
    ]
}

#[test]
fn tokens_stop_with_the_device_and_the_password() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (phone, tablet) = (TestDevice::new(13), TestDevice::new(14));
    phone.register(&s, &token);
    tablet.register(&s, &token);
    // Adapted after review round 3 (R3-2): an added device manages others only once approved.
    phone.approve(&s, &tablet);
    let (phone_token, tablet_token) = (phone.session(&s), tablet.session(&s));
    // An account token is no device token: no change of devices with the password alone.
    assert_eq!(device_calls(&s, &token, &phone.id()), vec![401; 4]);
    // Removal: the removed device's token is dead for everything, at once.
    let body = json!({"device_id": phone.id()});
    assert_eq!(
        s.post("/v1/device/remove", Some(&tablet_token), &body)
            .status,
        200
    );
    assert!(!works(&s, &phone_token));
    assert_eq!(device_calls(&s, &phone_token, &tablet.id()), vec![401; 4]);
    assert_eq!(phone.session_reply(&s).status, 410);
    // Password change: every earlier token of the account is dead, the new one works.
    let change = json!({"old_password": PASSWORD, "new_password": "new-password-1"});
    let changed = s.post("/v1/account/password", Some(&token), &change);
    assert_eq!(changed.status, 200);
    let fresh: Value = changed.json();
    assert!(!works(&s, &token) && !works(&s, &tablet_token));
    assert!(works(&s, fresh["token"].as_str().unwrap()));
    assert_eq!(device_calls(&s, &tablet_token, &tablet.id()), vec![401; 4]);
    assert!(works(&s, &tablet.session(&s)), "the key signs in again");
}

#[test]
fn a_device_token_reaches_no_device_of_another_account() {
    let s = TestServer::start();
    let (anna, mallory) = (s.register("anna"), s.register("mallory"));
    let (victim, own) = (TestDevice::new(15), TestDevice::new(16));
    let number = victim.number(&s, &anna);
    own.register(&s, &mallory);
    let own_token = own.session(&s);
    let named = json!({"device_id": victim.id()});
    let rename = json!({"device_id": victim.id(), "name": "owned"});
    for (path, body) in [
        ("/v1/device/remove", &named),
        ("/v1/device/rename", &rename),
    ] {
        let reply = s.post(path, Some(&own_token), body);
        assert_eq!(code(&reply), (404, "device_unknown".into()), "{path}");
    }
    let list = s.get_as("/v1/devices", &own_token).json();
    assert_eq!(list["devices"].as_array().unwrap().len(), 1);
    assert_eq!(victim.session_reply(&s).json()["number"], number.as_str());
    // A device token is not accepted where the account token is required (legacy upgrade).
    let c = own.challenge(&s, "upgrade", Some(&own_token));
    let body = json!({
        "legacy_device_id": "x",
        "challenge_id": c.id,
        "signature": own.sign("ultar-device-upgrade-v1", &c, b"x"),
    });
    let reply = s.post("/v1/device/upgrade", Some(&own_token), &body);
    assert_eq!(code(&reply), (401, "auth_required".into()));
}
