//! Security review r3 (device identity): race conditions. Requests are prepared (challenge and
//! signatures) on one thread and then fired at the same instant from several threads. The
//! invariants of `docs/DEVICE-IDENTITY.md` §6 must hold whatever the interleaving.

mod common;
use common::device::{Challenge, TestDevice};
use common::{request, TestServer};
use serde_json::{json, Value};
use std::sync::Barrier;

const REPLACE_OLD: &str = "ultar-device-replace-old-v1";
const REPLACE_NEW: &str = "ultar-device-replace-new-v1";
const RECOVER: &str = "ultar-device-recover-v1";

/// A request ready to be sent: path, bearer token, body.
type Shot = (String, String, Value);

/// Sends all shots at once; returns `(status, error code or "")` in the order of the shots.
fn volley(base: &str, shots: &[Shot]) -> Vec<(u16, String)> {
    let barrier = Barrier::new(shots.len());
    std::thread::scope(|scope| {
        let handles: Vec<_> = shots
            .iter()
            .map(|(path, token, body)| {
                let barrier = &barrier;
                scope.spawn(move || {
                    let url = format!("{base}{path}");
                    let bearer = format!("Bearer {token}");
                    let headers = [
                        ("content-type", "application/json"),
                        ("authorization", bearer.as_str()),
                    ];
                    barrier.wait();
                    let reply = request("POST", &url, &headers, Some(&body.to_string()));
                    let error = if reply.status == 200 {
                        String::new()
                    } else {
                        reply.error_code()
                    };
                    (reply.status, error)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

fn count(s: &TestServer, sql: &str) -> i64 {
    s.state.db.lock().query_row(sql, [], |r| r.get(0)).unwrap()
}

/// §6: an active number belongs to exactly one active device, and no device has two.
fn assert_numbers_consistent(s: &TestServer) {
    let orphans = count(
        s,
        "SELECT COUNT(*) FROM numbers n JOIN devices d ON d.id = n.device_row
         WHERE n.state = 1 AND d.state NOT IN (0, 1)",
    );
    assert_eq!(orphans, 0, "an active number on a retired device");
    let doubles = count(
        s,
        "SELECT COUNT(*) FROM (SELECT device_row FROM numbers WHERE state = 1
         GROUP BY device_row HAVING COUNT(*) > 1)",
    );
    assert_eq!(doubles, 0, "a device with two active numbers");
    let bare = count(
        s,
        "SELECT COUNT(*) FROM devices d WHERE d.state = 1 AND NOT EXISTS
         (SELECT 1 FROM numbers n WHERE n.device_row = d.id AND n.state = 1)",
    );
    assert_eq!(bare, 0, "an active device without a number");
}

fn register_shot(s: &TestServer, token: &str, device: &TestDevice) -> Shot {
    let c = device.challenge(s, "register", Some(token));
    let sig = device.sign("ultar-device-register-v1", &c, b"");
    let body = json!({"challenge_id": c.id, "signature": sig, "name": "race"});
    ("/v1/device/register".into(), token.to_string(), body)
}

#[test]
fn one_key_registered_eight_times_at_once_is_one_device() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(1);
    let shots: Vec<Shot> = (0..8).map(|_| register_shot(&s, &token, &device)).collect();
    let results = volley(&s.base, &shots);
    assert!(results.iter().all(|r| r.0 == 200), "{results:?}");
    assert_eq!(count(&s, "SELECT COUNT(*) FROM devices"), 1);
    assert_eq!(count(&s, "SELECT COUNT(*) FROM numbers"), 1);
    assert_eq!(count(&s, "SELECT device_quota_used FROM accounts"), 1);
    assert_numbers_consistent(&s);
}

#[test]
fn one_challenge_presented_eight_times_at_once_is_accepted_once() {
    let s = TestServer::start();
    let token = s.register("anna");
    let device = TestDevice::new(2);
    let shot = register_shot(&s, &token, &device);
    let shots = vec![shot; 8];
    let results = volley(&s.base, &shots);
    let accepted = results.iter().filter(|r| r.0 == 200).count();
    let used = results.iter().filter(|r| r.1 == "challenge_used").count();
    assert_eq!((accepted, used), (1, 7), "{results:?}");
}

#[test]
fn the_device_limit_holds_under_parallel_registrations() {
    let s = TestServer::start_with(|c| c.devices.max_devices = 3);
    let token = s.register("anna");
    let devices: Vec<TestDevice> = (10..17).map(TestDevice::new).collect();
    let shots: Vec<Shot> = devices
        .iter()
        .map(|d| register_shot(&s, &token, d))
        .collect();
    let results = volley(&s.base, &shots);
    let accepted = results.iter().filter(|r| r.0 == 200).count();
    let limited = results.iter().filter(|r| r.1 == "device_limit").count();
    assert_eq!((accepted, limited), (3, 4), "{results:?}");
    assert_eq!(count(&s, "SELECT COUNT(*) FROM devices WHERE state=1"), 3);
    assert_numbers_consistent(&s);
}

fn replace_shot(s: &TestServer, token: &str, old: &TestDevice, new: &TestDevice) -> Shot {
    let c: Challenge = new.challenge(s, "replace", Some(token));
    let body = json!({
        "challenge_id": c.id,
        "old_device_id": old.id(),
        "old_signature": old.sign(REPLACE_OLD, &c, &new.public()),
        "new_signature": new.sign(REPLACE_NEW, &c, &old.public()),
        "name": "new",
    });
    ("/v1/device/replace".into(), token.to_string(), body)
}

#[test]
fn two_replacements_of_one_device_at_once_move_the_number_once() {
    let s = TestServer::start();
    let token = s.register("anna");
    for round in 0..6u8 {
        s.clock.advance_millis(121_000);
        let old = TestDevice::new(20 + round * 3);
        let (first, second) = (
            TestDevice::new(21 + round * 3),
            TestDevice::new(22 + round * 3),
        );
        let number = old.number(&s, &token);
        let shots = [
            replace_shot(&s, &token, &old, &first),
            replace_shot(&s, &token, &old, &second),
        ];
        let results = volley(&s.base, &shots);
        let accepted = results.iter().filter(|r| r.0 == 200).count();
        assert_eq!(accepted, 1, "round {round}: {results:?}");
        let holders = count(
            &s,
            &format!("SELECT COUNT(*) FROM numbers WHERE number='{number}' AND state=1"),
        );
        assert_eq!(holders, 1);
        assert_numbers_consistent(&s);
        assert_eq!(old.session_reply(&s).status, 410, "the old key is retired");
    }
    assert_eq!(count(&s, "SELECT COUNT(*) FROM devices WHERE state=1"), 6);
}

#[test]
fn removal_and_replacement_of_one_device_at_once_never_both_succeed() {
    let s = TestServer::start();
    let token = s.register("anna");
    let keeper = TestDevice::new(90);
    keeper.register(&s, &token);
    let keeper_token = keeper.session(&s);
    let mut outcomes = std::collections::BTreeSet::new();
    for round in 0..8u8 {
        s.clock.advance_millis(121_000);
        let (old, new) = (
            TestDevice::new(100 + round * 2),
            TestDevice::new(101 + round * 2),
        );
        let number = old.number(&s, &token);
        let remove = (
            "/v1/device/remove".to_string(),
            keeper_token.clone(),
            json!({"device_id": old.id()}),
        );
        let results = volley(&s.base, &[remove, replace_shot(&s, &token, &old, &new)]);
        let both = results[0].0 == 200 && results[1].0 == 200;
        assert!(!both, "round {round}: removed and replaced: {results:?}");
        assert!(results.iter().any(|r| r.0 == 200), "{results:?}");
        assert_numbers_consistent(&s);
        assert_eq!(old.session_reply(&s).status, 410);
        outcomes.insert(results[0].0 == 200);
        if results[0].0 == 200 {
            // Removal won: the number is retired, the new key holds nothing.
            let sql = format!("SELECT COUNT(*) FROM numbers WHERE number='{number}' AND state=1");
            assert_eq!(count(&s, &sql), 0);
            assert_eq!(new.session_reply(&s).status, 404);
        } else {
            // The winner of the replacement is removed again to stay under the device limit.
            let body = json!({"device_id": new.id()});
            let reply = s.post("/v1/device/remove", Some(&keeper_token), &body);
            assert_eq!(reply.status, 200);
        }
    }
    println!("removal won in some rounds: {outcomes:?}");
}

fn recover_shot(s: &TestServer, token: &str, lost: &TestDevice, new: &TestDevice) -> Shot {
    let c = new.challenge(s, "recover", Some(token));
    let body = json!({
        "old_device_id": lost.id(),
        "challenge_id": c.id,
        // Adapted after review round 3: the end of a recovery has a label of its own.
        "signature": new.sign("ultar-device-recover-finish-v1", &c, lost.id().as_bytes()),
        "name": "recovered",
    });
    ("/v1/device/recover/finish".into(), token.to_string(), body)
}

#[test]
fn a_recovery_finished_twice_at_once_moves_the_number_once() {
    let s = TestServer::start_with(|c| c.devices.recovery_cooldown_secs = 3600);
    let token = s.register("anna");
    let (lost, new) = (TestDevice::new(200), TestDevice::new(201));
    let number = lost.number(&s, &token);
    let c = new.challenge(&s, "recover", Some(&token));
    let start = json!({
        "password": "password-123",
        "old_device_id": lost.id(),
        "challenge_id": c.id,
        "signature": new.sign(RECOVER, &c, lost.id().as_bytes()),
    });
    let started = s.post("/v1/device/recover/start", Some(&token), &start);
    assert_eq!(started.status, 200, "{}", started.text());
    s.clock.advance_millis(3_600_000);
    let shots: Vec<Shot> = (0..4)
        .map(|_| recover_shot(&s, &token, &lost, &new))
        .collect();
    let results = volley(&s.base, &shots);
    assert_eq!(
        results.iter().filter(|r| r.0 == 200).count(),
        1,
        "{results:?}"
    );
    assert_eq!(count(&s, "SELECT COUNT(*) FROM devices WHERE state=1"), 1);
    assert_eq!(count(&s, "SELECT COUNT(*) FROM device_recoveries"), 0);
    let sql = format!("SELECT COUNT(*) FROM numbers WHERE number='{number}' AND state=1");
    assert_eq!(count(&s, &sql), 1);
    assert_numbers_consistent(&s);
}
