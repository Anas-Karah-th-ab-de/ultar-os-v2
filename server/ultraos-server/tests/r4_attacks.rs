//! Security review r4: attacks on the new parts of the fixes of round 3 – approval of added
//! devices, the standing of an added device, one socket per number. All must fail; all pass on
//! the reviewed code.

mod common;
use common::device::{next_json, open_signal, runtime, send_json, Challenge, TestDevice};
use common::TestServer;
use serde_json::json;

const APPROVE: &str = "ultar-device-approve-v1";
const COOLDOWN_MS: u64 = 72 * 3600 * 1000;

fn code(reply: &common::Reply) -> (u16, String) {
    (reply.status, reply.error_code())
}

fn approve_with(
    s: &TestServer,
    token: &str,
    target: &str,
    c: &Challenge,
    sig: &str,
) -> (u16, String) {
    let body = json!({"device_id": target, "challenge_id": c.id, "signature": sig});
    code(&s.post("/v1/device/approve", Some(token), &body))
}

fn approved(s: &TestServer, device: &TestDevice) -> bool {
    let conn = s.state.db.lock();
    conn.query_row(
        "SELECT approved FROM devices WHERE device_id=?1",
        [device.id()],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        == 1
}

#[test]
fn an_added_device_cannot_approve_itself_or_be_approved_by_a_borrowed_signature() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (phone, thief, other) = (TestDevice::new(1), TestDevice::new(2), TestDevice::new(3));
    phone.register(&s, &token);
    thief.register(&s, &token);
    other.register(&s, &token);
    let thief_token = thief.session(&s);
    // Self-approval and approval of another added device by an added device.
    for target in [&thief, &other] {
        let c = thief.challenge(&s, "approve", Some(&thief_token));
        let sig = thief.sign(APPROVE, &c, &target.public());
        assert_eq!(
            approve_with(&s, &thief_token, &target.id(), &c, &sig).0,
            403
        );
    }
    // The thief's token with the owner's key: a signature the phone made for another device,
    // and the phone's signature presented under the thief's token.
    let phone_token = phone.session(&s);
    let c = phone.challenge(&s, "approve", Some(&phone_token));
    let for_other = phone.sign(APPROVE, &c, &other.public());
    assert_eq!(
        approve_with(&s, &phone_token, &thief.id(), &c, &for_other),
        (401, "signature_invalid".into())
    );
    let c = phone.challenge(&s, "approve", Some(&token));
    let sig = phone.sign(APPROVE, &c, &thief.public());
    assert_eq!(approve_with(&s, &thief_token, &thief.id(), &c, &sig).0, 400);
    // Every other label over the right extra at the approve endpoint.
    for label in [
        "ultar-device-register-v1",
        "ultar-device-session-v1",
        "ultar-device-replace-old-v1",
        "ultar-device-recover-v1",
    ] {
        let c = phone.challenge(&s, "approve", Some(&phone_token));
        let sig = phone.sign(label, &c, &thief.public());
        assert_eq!(approve_with(&s, &phone_token, &thief.id(), &c, &sig).0, 401);
    }
    assert!(!approved(&s, &thief) && !approved(&s, &other));
}

#[test]
fn an_approval_is_single_use_and_stays_in_its_account() {
    let s = TestServer::start();
    let (anna, bert) = (s.register("anna"), s.register("bert"));
    let (phone, tablet, foreign) = (TestDevice::new(4), TestDevice::new(5), TestDevice::new(6));
    phone.register(&s, &anna);
    tablet.register(&s, &anna);
    foreign.register(&s, &bert);
    let phone_token = phone.session(&s);
    let c = phone.challenge(&s, "approve", Some(&phone_token));
    let sig = phone.sign(APPROVE, &c, &foreign.public());
    // A device of another account cannot be approved (and the answer is that of an unknown id).
    assert_eq!(
        approve_with(&s, &phone_token, &foreign.id(), &c, &sig),
        (404, "device_unknown".into())
    );
    let c = phone.challenge(&s, "approve", Some(&phone_token));
    let sig = phone.sign(APPROVE, &c, &tablet.public());
    assert_eq!(
        approve_with(&s, &phone_token, &tablet.id(), &c, &sig).0,
        200
    );
    assert_eq!(
        approve_with(&s, &phone_token, &tablet.id(), &c, &sig),
        (400, "challenge_used".into())
    );
    // An approve signature does not register, start a session or open anything else.
    let c = phone.challenge(&s, "session", None);
    let sig = phone.sign(APPROVE, &c, b"");
    let body = json!({"challenge_id": c.id, "signature": sig});
    assert_eq!(s.post("/v1/device/session", None, &body).status, 401);
}

#[test]
fn an_added_device_gets_nothing_by_replacing_or_removing_itself() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (phone, thief, thief2) = (TestDevice::new(7), TestDevice::new(8), TestDevice::new(9));
    phone.register(&s, &token);
    thief.register(&s, &token);
    s.clock.advance_millis(COOLDOWN_MS - 3_600_000);
    // One hour before its own cool-down ends the thief moves to a fresh key by replacement.
    let c = thief2.challenge(&s, "replace", Some(&token));
    let body = json!({
        "challenge_id": c.id,
        "old_device_id": thief.id(),
        "old_signature": thief.sign("ultar-device-replace-old-v1", &c, &thief2.public()),
        "new_signature": thief2.sign("ultar-device-replace-new-v1", &c, &thief.public()),
    });
    assert_eq!(
        s.post("/v1/device/replace", Some(&token), &body).status,
        200
    );
    let thief2_token = thief2.session(&s);
    let named = json!({"device_id": phone.id()});
    let remove = |t: &str| s.post("/v1/device/remove", Some(t), &named).status;
    assert_eq!(remove(&thief2_token), 403, "no standing gained");
    s.clock.advance_millis(3_600_000 - 1_000);
    assert_eq!(remove(&thief2.session(&s)), 403, "cool-down not restarted…");
    assert_eq!(phone.session_reply(&s).status, 200);
    s.clock.advance_millis(2_000);
    // …nor shortened: the clock of the first key goes on (§10: after 72 h, as designed).
    assert_eq!(remove(&thief2.session(&s)), 200);
}

#[test]
fn a_stranger_cannot_push_the_owner_off_the_signalling_socket() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (phone, stranger) = (TestDevice::new(10), TestDevice::new(11));
    let number = phone.number(&s, &token);
    stranger.register(&s, &token);
    runtime().block_on(async {
        let (mut owner, ready) = phone.signal(&s).await;
        assert_eq!(ready["type"], "ready");
        // Same account, own key, but the phone's id: refused, the owner stays.
        let (_ws, refused) = phone.signal_signed_by(&s, &stranger).await;
        assert_eq!(refused["error"], "auth");
        // Hello for the phone's id without ever answering the challenge.
        let mut idle = open_signal(&s).await;
        send_json(&mut idle, &json!({"type":"hello","device_id": phone.id()})).await;
        assert_eq!(next_json(&mut idle).await["type"], "challenge");
        assert!(s.state.hub.is_online(&number));
        send_json(&mut owner, &json!({"to": number, "type": "ice"})).await;
        assert_eq!(next_json(&mut owner).await["from"], number.as_str());
    });
}
