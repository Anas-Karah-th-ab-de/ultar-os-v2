//! Migration of registrations from before device keys (schema 1 → 2).
//!
//! `fixtures/v1-devices.sql` is the dump of a database **written by the server code of schema 1**
//! (the repository state before the device-identity change): that server binary was started and
//! three accounts (`fixture-anna`, `fixture-mallory`, `fixture-bert`, password `fixture-pass-1`,
//! token key `11…11`) were created through its HTTP API. Anna registered two devices with the old
//! FNV ids. Mallory registered `7af8c5b4c52e91a3` – the id every device without an Android serial
//! number computed (`short_id("unknown", "")`) – and Bert's serial-less device was then refused
//! by that server with `device_taken`: the squatting of finding S15. The dump was taken with
//! SQLite's `.dump` (text, so that it can travel in a patch).

mod common;
use common::device::{next_json, open_signal, runtime, send_json, TestDevice};
use common::{TestServer, SECRET};
use serde_json::json;
use ultraos_server::accounts::TOKEN_TTL_SECS;
use ultraos_server::auth;

const ANNA_PHONE: (&str, &str) = ("a3f497b589a474e9", "UOS-D728-EDAD");
const ANNA_TABLET: (&str, &str) = ("a1b04767f0484668", "UOS-857F-8E06");
const CONSTANT_ID: &str = "7af8c5b4c52e91a3";
const MALLORY_NUMBER: &str = "UOS-8066-6C00";
const UPGRADE: &str = "ultar-device-upgrade-v1";

/// Loads the dump into a fresh file and starts the current server on it.
fn start() -> TestServer {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("live.sqlite");
    let dump = include_str!("fixtures/v1-devices.sql");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(dump).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1, "the fixture really is schema 1");
        let old_shape: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('devices') WHERE name='pubkey'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old_shape, 1);
    }
    TestServer::start_in(dir, &db, |_| {})
}

/// An account token as a device still holds it from before the migration (issued 100 s ago).
fn old_token(account_id: i64) -> String {
    auth::issue_token(&SECRET, account_id, TOKEN_TTL_SECS - 100).unwrap()
}

fn login(s: &TestServer, user: &str) -> String {
    let r = s.post(
        "/v1/account/login",
        None,
        &json!({"username": user, "password": "fixture-pass-1"}),
    );
    assert_eq!(r.status, 200, "{}", r.text());
    r.json()["token"].as_str().unwrap().to_string()
}

fn upgrade(s: &TestServer, token: &str, legacy_id: &str, device: &TestDevice) -> common::Reply {
    let c = device.challenge(s, "upgrade", Some(token));
    s.post(
        "/v1/device/upgrade",
        Some(token),
        &json!({"legacy_device_id": legacy_id, "challenge_id": c.id, "name": "upgraded",
                "signature": device.sign(UPGRADE, &c, legacy_id.as_bytes())}),
    )
}

fn code(reply: &common::Reply) -> (u16, String) {
    (reply.status, reply.error_code())
}

#[test]
fn old_registrations_become_legacy_rows_and_keep_their_numbers() {
    let s = start();
    let version: i64 = s
        .state
        .db
        .lock()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 3);
    let anna = old_token(1);
    let list = s.get_as("/v1/devices", &anna).json();
    let devices = list["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0]["device_id"], format!("legacy:{}", ANNA_PHONE.0));
    assert_eq!(
        (devices[0]["state"].as_str(), devices[0]["number"].as_str()),
        (Some("legacy"), Some(ANNA_PHONE.1))
    );
    assert_eq!(devices[1]["number"], ANNA_TABLET.1);
    assert_eq!(devices[0]["last_seen"], 0);
    // The free text that was stored as "pubkey" is gone.
    let keys: i64 = s
        .state
        .db
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM devices WHERE public_key IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(keys, 0);
    first_release_clients_can_only_read_their_number(&s, &anna);
}

/// Second half of the test above.
fn first_release_clients_can_only_read_their_number(s: &TestServer, anna: &str) {
    // A device that still runs the first release can read its number, and nothing else.
    let old_form = |token: &str, id: &str| {
        s.post(
            "/v1/device/register",
            Some(token),
            &json!({"device_id": id, "pubkey": ""}),
        )
    };
    assert_eq!(old_form(anna, ANNA_PHONE.0).json()["number"], ANNA_PHONE.1);
    assert_eq!(
        code(&old_form(anna, "brand-new-id")),
        (426, "device_key_required".into())
    );
    assert_eq!(
        code(&old_form(anna, CONSTANT_ID)),
        (426, "device_key_required".into()),
        "another account's id"
    );
    // A legacy registration has no key: no signalling socket, by no means.
    runtime().block_on(async {
        for hello in [
            json!({"token": anna, "number": ANNA_PHONE.1}),
            json!({"type":"hello","device_id": format!("legacy:{}", ANNA_PHONE.0)}),
            json!({"type":"hello","device_id": ANNA_PHONE.0}),
        ] {
            let mut ws = open_signal(s).await;
            send_json(&mut ws, &hello).await;
            let answer = next_json(&mut ws).await;
            assert_eq!(answer["type"], "error", "{hello}");
        }
    });
}

#[test]
fn a_legacy_device_upgrades_in_place_with_the_token_it_already_had() {
    let s = start();
    let anna = old_token(1);
    let phone = TestDevice::new(1);
    let reply = upgrade(&s, &anna, ANNA_PHONE.0, &phone);
    assert_eq!(reply.status, 200, "{}", reply.text());
    let v = reply.json();
    assert_eq!(v["number"], ANNA_PHONE.1, "the number stays");
    assert_eq!(v["device_id"], phone.id().as_str());
    let list = s.get_as("/v1/devices", v["token"].as_str().unwrap()).json();
    let first = &list["devices"][0];
    assert_eq!(
        (first["state"].as_str(), first["name"].as_str()),
        (Some("active"), Some("upgraded"))
    );
    assert_eq!(first["current"], true);
    assert_eq!(
        list["devices"].as_array().unwrap().len(),
        2,
        "same row, no extra device"
    );
    // From now on the device works like any keyed device.
    assert_eq!(phone.session_reply(&s).json()["number"], ANNA_PHONE.1);
    runtime().block_on(async {
        let (_ws, ready) = phone.signal(&s).await;
        assert_eq!(ready["number"], ANNA_PHONE.1);
    });
    // The legacy id is used up: a second key cannot upgrade it, the old form no longer finds it.
    assert_eq!(
        code(&upgrade(&s, &anna, ANNA_PHONE.0, &TestDevice::new(2))),
        (403, "legacy_not_eligible".into())
    );
    let old_form = s.post(
        "/v1/device/register",
        Some(&anna),
        &json!({"device_id": ANNA_PHONE.0, "pubkey": ""}),
    );
    assert_eq!(code(&old_form), (426, "device_key_required".into()));
    // The other legacy device of the account is untouched and can upgrade too.
    assert_eq!(
        upgrade(&s, &anna, ANNA_TABLET.0, &TestDevice::new(3)).json()["number"],
        ANNA_TABLET.1
    );
}

#[test]
fn upgrade_is_refused_without_the_old_token_of_the_owning_account() {
    let s = start();
    let device = TestDevice::new(1);
    let refused = (403, "legacy_not_eligible".to_string());
    // A token obtained after the migration (a password thief logging in now) does not qualify.
    let fresh = login(&s, "fixture-anna");
    assert_eq!(code(&upgrade(&s, &fresh, ANNA_PHONE.0, &device)), refused);
    // Another account, even with a token from before the migration.
    assert_eq!(
        code(&upgrade(&s, &old_token(2), ANNA_PHONE.0, &device)),
        refused
    );
    assert_eq!(
        code(&upgrade(&s, &old_token(3), ANNA_PHONE.0, &device)),
        refused
    );
    // Unknown legacy ids look the same.
    assert_eq!(
        code(&upgrade(&s, &old_token(1), "0000000000000000", &device)),
        refused
    );
    // The proof must name the legacy id that is upgraded.
    let anna = old_token(1);
    let c = device.challenge(&s, "upgrade", Some(&anna));
    let r = s.post(
        "/v1/device/upgrade",
        Some(&anna),
        &json!({"legacy_device_id": ANNA_PHONE.0, "challenge_id": c.id,
                "signature": device.sign(UPGRADE, &c, ANNA_TABLET.0.as_bytes())}),
    );
    assert_eq!(code(&r), (401, "signature_invalid".into()));
    // Nothing changed for anna.
    let list = s.get_as("/v1/devices", &anna).json();
    assert!(list["devices"]
        .as_array()
        .unwrap()
        .iter()
        .all(|d| d["state"] == "legacy"));
    // The fresh login still gets the device a NEW registration, and the owner can move the old
    // number with password + cool-down (the lost-device path) if she wants it back.
    let registered = device.register(&s, &fresh);
    assert_ne!(registered["number"], ANNA_PHONE.1);
}

/// The squatted id of the review: Mallory holds the constant id of serial-less devices. After the
/// migration Bert's device is no longer blocked, cannot be confused with Mallory's row, and
/// neither of them can take the other's number.
#[test]
fn the_squatted_constant_id_blocks_nobody_and_cannot_be_taken_over() {
    let s = start();
    let (mallory, bert) = (old_token(2), old_token(3));
    let berts_device = TestDevice::new(7);

    // Bert's updated device first tries to upgrade "its" legacy id – the one Mallory squatted.
    assert_eq!(
        code(&upgrade(&s, &bert, CONSTANT_ID, &berts_device)),
        (403, "legacy_not_eligible".into())
    );
    // It then registers with its key and finally gets a number of its own.
    let registered = berts_device.register(&s, &bert);
    let number = registered["number"].as_str().unwrap();
    assert_ne!(number, MALLORY_NUMBER);
    assert_eq!(registered["device_id"], berts_device.id().as_str());
    runtime().block_on(async {
        let (_ws, ready) = berts_device.signal(&s).await;
        assert_eq!(ready["number"], number);
    });
    // A second serial-less device (same old constant id) gets yet another id and number.
    let second = TestDevice::new(8).register(&s, &bert);
    assert_ne!(second["device_id"], registered["device_id"]);

    // Mallory can no longer squat anything: not Bert's key id, not a new legacy id.
    let squat = s.post(
        "/v1/device/register",
        Some(&mallory),
        &json!({"device_id": berts_device.id(), "pubkey": berts_device.public_b64()}),
    );
    assert_eq!(code(&squat), (426, "device_key_required".into()));
    assert_eq!(
        code(&berts_device.register_reply(&s, &mallory, "")),
        (409, "device_taken".into())
    );
    // Her own legacy row is still hers (she can upgrade it to her own key); it is nobody else's.
    let hers = upgrade(&s, &mallory, CONSTANT_ID, &TestDevice::new(66));
    assert_eq!(hers.json()["number"], MALLORY_NUMBER);
    let berts = s.get_as("/v1/devices", &bert).json();
    assert_eq!(berts["devices"].as_array().unwrap().len(), 2);
}

#[test]
fn a_legacy_number_can_be_recovered_with_password_and_cool_down() {
    let s = start();
    let device = TestDevice::new(1);
    let lost = format!("legacy:{}", ANNA_PHONE.0);
    let proof = |token: &str, path: &str, label: &str, extra: serde_json::Value| {
        let c = device.challenge(&s, "recover", Some(token));
        let mut body = json!({"old_device_id": lost, "challenge_id": c.id,
            "signature": device.sign(label, &c, lost.as_bytes())});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        s.post(path, Some(token), &body)
    };
    let (start_path, finish_path) = ("/v1/device/recover/start", "/v1/device/recover/finish");
    let (start_label, finish_label) = ("ultar-device-recover-v1", "ultar-device-recover-finish-v1");
    let password = json!({"password": "fixture-pass-1"});
    // A legacy registration cannot be told about a recovery. While its own device can still
    // upgrade it with the old account token (30 days), the password alone does not move it.
    let fresh = login(&s, "fixture-anna");
    let locked = proof(&fresh, start_path, start_label, password.clone());
    assert_eq!(code(&locked), (409, "recovery_locked".into()));
    let wait: i64 = locked.header("retry-after").unwrap().parse().unwrap();
    assert!((30 * 86_400 - 5..=30 * 86_400).contains(&wait), "{wait}");
    s.clock.advance_millis(30 * 86_400 * 1000);
    let fresh = login(&s, "fixture-anna");
    assert_eq!(proof(&fresh, start_path, start_label, password).status, 200);
    let list = s.get_as("/v1/devices", &fresh).json();
    assert_eq!(list["recovery"]["old_device"], lost.as_str());
    assert_eq!(list["recovery"]["eligible_in"], 72 * 3600);
    assert_eq!(
        code(&proof(&fresh, finish_path, finish_label, json!({}))),
        (409, "cooldown_active".into())
    );
    s.clock.advance_millis(72 * 3_600_000);
    // The start label does not finish a recovery.
    assert_eq!(
        code(&proof(&fresh, finish_path, start_label, json!({}))),
        (401, "signature_invalid".into())
    );
    let done = proof(&fresh, finish_path, finish_label, json!({}));
    assert_eq!(done.json()["number"], ANNA_PHONE.1);
    assert_eq!(done.json()["manage"], true);
}

#[test]
fn restart_on_the_migrated_file_changes_nothing() {
    let s = start();
    let anna = old_token(1);
    assert_eq!(
        upgrade(&s, &anna, ANNA_PHONE.0, &TestDevice::new(1)).status,
        200
    );
    let path = s.dir.path().join("live.sqlite");
    let cutoff = |conn: &rusqlite::Connection| -> String {
        conn.query_row(
            "SELECT value FROM meta WHERE key='legacy_cutoff'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    let before = cutoff(&s.state.db.lock());
    let again = ultraos_server::db::Db::open(&path.to_string_lossy()).unwrap();
    let conn = again.lock();
    assert_eq!(cutoff(&conn), before, "the cut-off is set once");
    let (devices, numbers, keyed): (i64, i64, i64) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM devices), (SELECT COUNT(*) FROM numbers),
                    (SELECT COUNT(*) FROM devices WHERE public_key IS NOT NULL)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((devices, numbers, keyed), (3, 3, 1));
}
