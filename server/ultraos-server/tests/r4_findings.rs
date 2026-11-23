//! Security review r4 (device identity, re-review of the fixes of round 3): defects that are
//! still open or were introduced by the fixes. Each test FAILS on the reviewed code; details in
//! the review report (R4-1 … R4-3).

mod common;
use common::device::TestDevice;
use common::TestServer;
use serde_json::json;
use ultraos_server::challenge::{ChallengeStore, ChallengeUse, MAX_PER_OWNER};

const PASSWORD: &str = "password-123";

fn login(s: &TestServer, user: &str, password: &str) -> (u16, String) {
    let body = json!({"username": user, "password": password});
    let reply = s.post("/v1/account/login", None, &body);
    let error = if reply.status == 200 {
        String::new()
    } else {
        reply.error_code()
    };
    (reply.status, error)
}

/// R4-1 (major). The per-account password budget (fix of R3-5) refuses *every* check once ten
/// failures are booked – the owner's right password included. Whoever knows the user name (no
/// token, no password) keeps the owner out of login, password change and recovery with ten
/// requests per 15 minutes from any address. That also blocks the owner's own way back after
/// losing his devices, and the password change that would stop a thief's recovery.
#[test]
fn r4_1_a_stranger_cannot_lock_the_owner_out_of_his_password() {
    let s = TestServer::start();
    let token = s.register("anna");
    let phone = TestDevice::new(1);
    phone.register(&s, &token);
    for guess in 0..10 {
        let (status, _) = login(&s, "anna", &format!("guess-{guess}-wrong"));
        assert_eq!(status, 401);
    }
    // The owner, with the right password, from another address a minute later.
    s.clock.advance_millis(60_000);
    assert_eq!(
        login(&s, "anna", PASSWORD).0,
        200,
        "the owner's right password is refused because a stranger guessed wrong"
    );
}

/// R4-1, second face: the owner lost his phone and starts the designed recovery; a stranger who
/// knows only the user name keeps it from starting.
#[test]
fn r4_1_a_stranger_cannot_block_the_owners_recovery() {
    let s = TestServer::start();
    let token = s.register("anna");
    let (lost, new) = (TestDevice::new(2), TestDevice::new(3));
    lost.register(&s, &token);
    for guess in 0..10 {
        login(&s, "anna", &format!("guess-{guess}-wrong"));
    }
    let c = new.challenge(&s, "recover", Some(&token));
    let body = json!({
        "password": PASSWORD,
        "old_device_id": lost.id(),
        "challenge_id": c.id,
        "signature": new.sign("ultar-device-recover-v1", &c, lost.id().as_bytes()),
    });
    let reply = s.post("/v1/device/recover/start", Some(&token), &body);
    assert_eq!(reply.status, 200, "{}", reply.text());
}

/// R4-2 (major, introduced by the fix of R3-6). A presented challenge frees its owner's slot at
/// once but stays in the store until it expires. The per-owner bound therefore no longer bounds
/// what one owner keeps in the store: request + present, again and again. One account (or one
/// device's key) fills the global store; then nobody can register or get a device token – and
/// without a device token nobody can cancel a recovery ("This was not me").
#[test]
fn r4_2_one_owner_cannot_fill_the_challenge_store() {
    let store = ChallengeStore::new(120_000, MAX_PER_OWNER, 64);
    let (mallory, anna) = ([1u8; 32], [2u8; 32]);
    for _ in 0..64 {
        let c = store.issue(ChallengeUse::Register, 7, mallory, 0).unwrap();
        let _ = store.take(&c.id, ChallengeUse::Register, Some(7), 0);
    }
    assert!(
        store.issue(ChallengeUse::Session, 9, anna, 1_000).is_ok(),
        "one account's presented challenges ({} stored) lock every other device out",
        store.len()
    );
}

/// R4-2 over HTTP: what one account leaves in the store after 200 request/present rounds.
#[test]
fn r4_2_presented_challenges_count_against_their_owner() {
    let s = TestServer::start();
    let token = s.register("mallory");
    let key = TestDevice::new(4);
    for _ in 0..200 {
        let c = key.challenge(&s, "register", Some(&token));
        let garbage = device_signature_of_zeros();
        let body = json!({"challenge_id": c.id, "signature": garbage, "name": ""});
        s.post("/v1/device/register", Some(&token), &body);
    }
    let stored = s.state.devices.challenges.len();
    assert!(stored > 0);
    assert!(
        stored <= 2 * MAX_PER_OWNER as usize,
        "one account holds {stored} entries of the global store"
    );
}

/// R4-3 (minor). Session challenges of a key are no longer refused but the oldest pending one is
/// dropped (fix of R3-6). Whoever knows a device's public key (no token needed) drops the
/// device's own challenge between its two requests with eight requests: the device's sign-in
/// fails with `challenge_unknown`. The lock-out of R3-6 needs eight requests per device round
/// trip now instead of eight per two minutes.
#[test]
fn r4_3_a_stranger_cannot_drop_a_devices_pending_sign_in() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(5);
    device.register(&s, &token);
    let mine = device.challenge(&s, "session", None);
    for _ in 0..MAX_PER_OWNER {
        s.clock.advance_millis(10);
        assert_eq!(device.challenge_reply(&s, "session", None).status, 200);
    }
    let signature = device.sign("ultar-device-session-v1", &mine, b"");
    let body = json!({"challenge_id": mine.id, "signature": signature});
    let reply = s.post("/v1/device/session", None, &body);
    assert_eq!(reply.status, 200, "{}", reply.text());
}

/// A well-formed signature that verifies for nothing.
fn device_signature_of_zeros() -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
    B64.encode([0u8; 64])
}
