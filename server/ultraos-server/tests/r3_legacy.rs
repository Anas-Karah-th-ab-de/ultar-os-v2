//! Security review r3 (device identity): abuse of registrations from before device keys after
//! the migration (schema 1 → 2), on the fixture written by the schema-1 server
//! (`fixtures/v1-devices.sql`, see `device_migration.rs`). All tests pass on the reviewed code;
//! the last one documents a residual risk (REPORT.md, R3-1).

mod common;
use common::device::TestDevice;
use common::{request, TestServer, SECRET};
use serde_json::json;
use std::sync::Barrier;
use ultraos_server::accounts::TOKEN_TTL_SECS;
use ultraos_server::auth;

const ANNA_PHONE: (&str, &str) = ("a3f497b589a474e9", "UOS-D728-EDAD");
const UPGRADE: &str = "ultar-device-upgrade-v1";
const RECOVER: &str = "ultar-device-recover-v1";
const PASSWORD: &str = "fixture-pass-1";

fn start() -> TestServer {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("live.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(include_str!("fixtures/v1-devices.sql"))
            .unwrap();
    }
    TestServer::start_in(dir, &db, |_| {})
}

/// The account token a device still holds from before the migration.
fn old_token(account_id: i64) -> String {
    auth::issue_token(&SECRET, account_id, TOKEN_TTL_SECS - 100).unwrap()
}

/// What a password thief gets after the migration.
fn login(s: &TestServer, user: &str) -> String {
    let body = json!({"username": user, "password": PASSWORD});
    let reply = s.post("/v1/account/login", None, &body);
    assert_eq!(reply.status, 200, "{}", reply.text());
    reply.json()["token"].as_str().unwrap().to_string()
}

fn upgrade_body(s: &TestServer, token: &str, id: &str, device: &TestDevice) -> serde_json::Value {
    let c = device.challenge(s, "upgrade", Some(token));
    json!({
        "legacy_device_id": id,
        "challenge_id": c.id,
        "name": "upgraded",
        "signature": device.sign(UPGRADE, &c, id.as_bytes()),
    })
}

fn upgrade(s: &TestServer, token: &str, id: &str, device: &TestDevice) -> (u16, String) {
    let reply = s.post(
        "/v1/device/upgrade",
        Some(token),
        &upgrade_body(s, token, id, device),
    );
    (reply.status, reply.error_code())
}

fn holder(s: &TestServer, number: &str) -> (String, i64) {
    let conn = s.state.db.lock();
    conn.query_row(
        "SELECT d.device_id, d.state FROM numbers n JOIN devices d ON d.id = n.device_row
         WHERE n.number=?1 AND n.state=1",
        [number],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

#[test]
fn the_legacy_namespace_cannot_be_entered_or_confused() {
    let s = start();
    let (anna, mallory) = (old_token(1), old_token(2));
    let key = TestDevice::new(1);
    let refused = (403, "legacy_not_eligible".to_string());
    // Other spellings of Anna's id are other ids; none of them is her registration.
    for id in [
        format!("legacy:{}", ANNA_PHONE.0),
        format!("{} ", ANNA_PHONE.0),
        ANNA_PHONE.0.to_uppercase(),
        format!("{}\u{0}", ANNA_PHONE.0),
        format!("{}%", &ANNA_PHONE.0[..8]),
    ] {
        assert_eq!(upgrade(&s, &anna, &id, &key), refused, "{id:?}");
    }
    // Mallory's old token is as old as Anna's, but the row is not hers.
    assert_eq!(upgrade(&s, &mallory, ANNA_PHONE.0, &key), refused);
    // Neither can she touch it through the calls that take a device id (she needs a device
    // token for them, so she registers a key of her own first).
    let own = TestDevice::new(2);
    own.register(&s, &mallory);
    // Adapted after review round 3 (R3-2): her added device may manage devices only after the
    // cool-down; before that the calls end with 403 whatever they name.
    let early = own.session(&s);
    let body = json!({"device_id": format!("legacy:{}", ANNA_PHONE.0), "name": "x"});
    assert_eq!(s.post("/v1/device/rename", Some(&early), &body).status, 403);
    s.clock.advance_millis(72 * 3600 * 1000);
    let own_token = own.session(&s);
    let target = format!("legacy:{}", ANNA_PHONE.0);
    for path in ["/v1/device/remove", "/v1/device/rename"] {
        let body = json!({"device_id": target, "name": "x"});
        assert_eq!(s.post(path, Some(&own_token), &body).status, 404, "{path}");
    }
    // The first-release register call creates nothing and reads nothing of another account.
    let old_form = json!({"device_id": ANNA_PHONE.0, "pubkey": "x"});
    let reply = s.post("/v1/device/register", Some(&mallory), &old_form);
    assert_eq!(reply.status, 426, "{}", reply.text());
    assert_eq!(
        holder(&s, ANNA_PHONE.1),
        (format!("legacy:{}", ANNA_PHONE.0), 0)
    );
}

#[test]
fn a_token_from_after_the_migration_never_upgrades() {
    let s = start();
    let key = TestDevice::new(3);
    let refused = (403, "legacy_not_eligible".to_string());
    // Stolen password, fresh login.
    let stolen = login(&s, "fixture-anna");
    assert_eq!(upgrade(&s, &stolen, ANNA_PHONE.0, &key), refused);
    // A token whose expiry says "issued in the second of the migration" or later.
    let cutoff: i64 = {
        let conn = s.state.db.lock();
        let text: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key='legacy_cutoff'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        text.parse().unwrap()
    };
    let now = ultraos_server::db::now();
    let at_cutoff = auth::issue_token(&SECRET, 1, TOKEN_TTL_SECS - (now - cutoff)).unwrap();
    assert_eq!(upgrade(&s, &at_cutoff, ANNA_PHONE.0, &key), refused);
    // An old token dies with a password change; it then upgrades nothing either.
    let old = old_token(1);
    let change = json!({"old_password": PASSWORD, "new_password": "another-pass-2"});
    assert_eq!(
        s.post("/v1/account/password", Some(&stolen), &change)
            .status,
        200
    );
    let body = json!({"purpose": "upgrade", "public_key": key.public_b64()});
    assert_eq!(
        s.post("/v1/device/challenge", Some(&old), &body).status,
        401
    );
    assert_eq!(
        holder(&s, ANNA_PHONE.1),
        (format!("legacy:{}", ANNA_PHONE.0), 0)
    );
}

#[test]
fn two_keys_upgrading_one_registration_at_once_get_one_number() {
    let s = start();
    let anna = old_token(1);
    let keys = [TestDevice::new(4), TestDevice::new(5), TestDevice::new(6)];
    let bodies: Vec<String> = keys
        .iter()
        .map(|k| upgrade_body(&s, &anna, ANNA_PHONE.0, k).to_string())
        .collect();
    let barrier = Barrier::new(bodies.len());
    let url = s.url("/v1/device/upgrade");
    let bearer = format!("Bearer {anna}");
    let statuses: Vec<u16> = std::thread::scope(|scope| {
        let handles: Vec<_> = bodies
            .iter()
            .map(|body| {
                let (barrier, url, bearer) = (&barrier, &url, &bearer);
                scope.spawn(move || {
                    let headers = [
                        ("content-type", "application/json"),
                        ("authorization", bearer.as_str()),
                    ];
                    barrier.wait();
                    request("POST", url, &headers, Some(body)).status
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(
        statuses.iter().filter(|c| **c == 200).count(),
        1,
        "{statuses:?}"
    );
    assert_eq!(
        statuses.iter().filter(|c| **c == 403).count(),
        2,
        "{statuses:?}"
    );
    let (id, state) = holder(&s, ANNA_PHONE.1);
    assert_eq!(state, 1);
    assert!(keys.iter().any(|k| k.id() == id));
    let conn = s.state.db.lock();
    let active: i64 = conn
        .query_row("SELECT COUNT(*) FROM devices WHERE state=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(active, 1);
}

/// Adapted after review round 3 (R3-1, legacy rows): a registration from before device keys
/// has neither a socket nor a device token – it cannot be told and cannot cancel. So a
/// password-only recovery of such a row is refused for as long as the owner's own device can
/// still take the row over with its old account token (30 days after the migration). After that
/// the row moves with password and cool-down as any other.
#[test]
fn a_legacy_number_moves_with_the_password_alone_after_the_cool_down() {
    let s = start();
    let stolen = login(&s, "fixture-anna");
    let thief = TestDevice::new(7);
    let lost = format!("legacy:{}", ANNA_PHONE.0);
    let proof = |s: &TestServer, token: &str, label: &str| {
        let c = thief.challenge(s, "recover", Some(token));
        (c.id.clone(), thief.sign(label, &c, lost.as_bytes()))
    };
    let (id, sig) = proof(&s, &stolen, RECOVER);
    let start =
        json!({"password": PASSWORD, "old_device_id": lost, "challenge_id": id, "signature": sig});
    let locked = s.post("/v1/device/recover/start", Some(&stolen), &start);
    assert_eq!(locked.status, 409, "{}", locked.text());
    assert_eq!(locked.error_code(), "recovery_locked");
    // Meanwhile the owner's device still upgrades in place with its old token.
    // 30 days later the lock is open (and the old tokens have expired).
    s.clock.advance_millis(30 * 86_400 * 1000);
    let stolen = login(&s, "fixture-anna");
    let (id, sig) = proof(&s, &stolen, RECOVER);
    let start =
        json!({"password": PASSWORD, "old_device_id": lost, "challenge_id": id, "signature": sig});
    assert_eq!(
        s.post("/v1/device/recover/start", Some(&stolen), &start)
            .status,
        200
    );
    // The owner's device: its account token from before the migration is all it has.
    let owner = old_token(1);
    assert_eq!(
        s.post("/v1/device/recover/cancel", Some(&owner), &json!({}))
            .status,
        401
    );
    assert_eq!(s.get_as("/v1/device/events", &owner).status, 401);
    s.clock.advance_millis(72 * 3600 * 1000);
    let (id, sig) = proof(&s, &stolen, "ultar-device-recover-finish-v1");
    let finish = json!({"old_device_id": lost, "challenge_id": id, "signature": sig});
    let done = s.post("/v1/device/recover/finish", Some(&stolen), &finish);
    assert_eq!(done.status, 200, "{}", done.text());
    assert_eq!(done.json()["number"], ANNA_PHONE.1);
    assert_eq!(holder(&s, ANNA_PHONE.1), (thief.id(), 1));
}
