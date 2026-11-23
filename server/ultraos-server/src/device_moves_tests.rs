use super::*;
use crate::db::Db;
use crate::devices::{list, register, remove};
use ed25519_dalek::SigningKey;

const LIMITS: Limits = Limits { max_devices: 3 };
const COOLDOWN: i64 = 1000;

fn db() -> Db {
    let db = Db::open(":memory:").unwrap();
    db.lock()
        .execute_batch(
            "INSERT INTO accounts(id, username, pw_hash, created) VALUES(1,'a','h',0),(2,'b','h',0);
             INSERT INTO devices(id, account_id, device_id, state, created) VALUES(50,1,'legacy:old-1',0,0);
             INSERT INTO numbers(number, account_id, device_row, state, created) VALUES('UOS-AAAA-0001',1,50,1,0);
             UPDATE meta SET value='5000' WHERE key='legacy_cutoff';",
        )
        .unwrap();
    db
}

fn key(seed: u8) -> [u8; 32] {
    SigningKey::from_bytes(&[seed; 32])
        .verifying_key()
        .to_bytes()
}

fn claim(account_id: i64, legacy_id: &str, token_issued: i64) -> LegacyClaim<'_> {
    LegacyClaim {
        account_id,
        legacy_id,
        token_issued,
    }
}

#[test]
fn legacy_upgrade_keeps_row_and_number_and_binds_the_key() {
    let db = db();
    let mut conn = db.lock();
    let done = upgrade_legacy(
        &mut conn,
        claim(1, "old-1", 4999),
        &key(1),
        "Phone",
        6000,
        LIMITS,
    )
    .unwrap();
    assert_eq!(
        (done.device_row, done.number.as_str()),
        (50, "UOS-AAAA-0001")
    );
    assert_eq!(done.device_id, identity::device_id(&key(1)));
    let row = devices::by_id(&conn, &done.device_id).unwrap().unwrap();
    assert_eq!(
        (row.state, row.public_key, row.epoch),
        (DeviceState::Active, Some(key(1)), 1)
    );
    assert!(
        devices::by_id(&conn, "legacy:old-1").unwrap().is_none(),
        "the legacy id is gone"
    );
    // A second upgrade of the same legacy id finds nothing.
    assert!(matches!(
        upgrade_legacy(
            &mut conn,
            claim(1, "old-1", 4999),
            &key(2),
            "",
            6000,
            LIMITS
        ),
        Err(DeviceError::LegacyNotEligible)
    ));
}

#[test]
fn legacy_upgrade_is_refused_for_everyone_but_the_owner_with_an_old_token() {
    let db = db();
    let mut conn = db.lock();
    let long = "x".repeat(500);
    for (who, id, issued) in [
        (2, "old-1", 4999),     // another account (the squatter's view, or the victim's)
        (1, "old-1", 5000),     // token issued in the second of the migration
        (1, "old-1", 5001),     // token issued after the migration
        (1, "unknown", 0),      // no such legacy id
        (1, "legacy:old-1", 0), // prefix games
        (1, long.as_str(), 0),
    ] {
        assert!(
            matches!(
                upgrade_legacy(&mut conn, claim(who, id, issued), &key(1), "", 6000, LIMITS),
                Err(DeviceError::LegacyNotEligible)
            ),
            "{who} {id} {issued}"
        );
    }
    let row = devices::by_id(&conn, "legacy:old-1").unwrap().unwrap();
    assert_eq!(
        (row.state, row.public_key),
        (DeviceState::Legacy, None),
        "untouched"
    );
    assert_eq!(legacy_cutoff(&conn).unwrap(), 5000);
    conn.execute("DELETE FROM meta", []).unwrap();
    assert_eq!(
        legacy_cutoff(&conn).unwrap(),
        i64::MIN,
        "no cut-off recorded: nobody qualifies"
    );
}

#[test]
fn upgrade_cannot_bind_a_key_that_is_known_already() {
    let db = db();
    let mut conn = db.lock();
    register(&mut conn, 2, &key(1), "", 0, LIMITS).unwrap();
    assert!(matches!(
        upgrade_legacy(&mut conn, claim(1, "old-1", 0), &key(1), "", 1, LIMITS),
        Err(DeviceError::Taken)
    ));
    let gone = register(&mut conn, 1, &key(2), "", 0, LIMITS).unwrap();
    remove(&mut conn, 1, &gone.device_id, 1).unwrap();
    assert!(matches!(
        upgrade_legacy(&mut conn, claim(1, "old-1", 0), &key(2), "", 1, LIMITS),
        Err(DeviceError::Retired)
    ));
}

#[test]
fn replace_moves_the_number_and_retires_the_old_key() {
    let db = db();
    let mut conn = db.lock();
    let old = register(&mut conn, 1, &key(1), "Old", 0, LIMITS).unwrap();
    let old_row = devices::by_id(&conn, &old.device_id).unwrap().unwrap();
    let new = replace(&mut conn, &old_row, &key(2), "New", 10, LIMITS).unwrap();
    assert_eq!(new.number, old.number);
    assert_ne!(new.device_row, old.device_row);
    let after = devices::by_id(&conn, &old.device_id).unwrap().unwrap();
    assert_eq!((after.state, after.epoch), (DeviceState::Replaced, 1));
    assert_eq!(devices::number_of(&conn, old.device_row).unwrap(), None);
    let names: Vec<String> = list(&conn, 1)
        .unwrap()
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(
        names,
        vec!["".to_string(), "New".to_string()],
        "legacy row and the new device"
    );
    // The same authorisation cannot be used twice, and the old key stays retired.
    assert!(matches!(
        replace(&mut conn, &old_row, &key(3), "", 11, LIMITS),
        Err(DeviceError::Unknown)
    ));
    assert!(matches!(
        register(&mut conn, 1, &key(1), "", 12, LIMITS),
        Err(DeviceError::Retired)
    ));
    let rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM numbers WHERE number=?1",
            [&old.number],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn replace_refuses_known_keys_and_does_not_need_a_free_slot() {
    let db = db();
    let mut conn = db.lock();
    let a = register(&mut conn, 1, &key(1), "", 0, LIMITS).unwrap();
    register(&mut conn, 1, &key(2), "", 0, LIMITS).unwrap();
    // account 1 now holds 3 devices in use (one legacy): the limit
    let a_row = devices::by_id(&conn, &a.device_id).unwrap().unwrap();
    assert!(matches!(
        replace(&mut conn, &a_row, &key(2), "", 1, LIMITS),
        Err(DeviceError::Taken)
    ));
    assert_eq!(
        devices::by_id(&conn, &a.device_id).unwrap().unwrap().state,
        DeviceState::Active,
        "a refused replacement changes nothing"
    );
    assert!(
        replace(&mut conn, &a_row, &key(3), "", 1, LIMITS).is_ok(),
        "one out, one in"
    );
}

#[test]
fn recovery_needs_the_cool_down_and_the_same_key() {
    let db = db();
    let mut conn = db.lock();
    let lost = register(&mut conn, 1, &key(1), "", 0, LIMITS).unwrap();
    let eligible = recovery_start(&mut conn, 1, &lost.device_id, &key(2), 100, COOLDOWN).unwrap();
    assert_eq!(eligible, 1100);
    assert_eq!(
        recovery_start(&mut conn, 1, &lost.device_id, &key(2), 500, COOLDOWN).unwrap(),
        1100,
        "asking again does not restart the clock"
    );
    assert!(matches!(
        recovery_start(&mut conn, 1, &lost.device_id, &key(3), 500, COOLDOWN),
        Err(DeviceError::RecoveryPending)
    ));
    assert!(matches!(
        recovery_finish(&mut conn, 1, &lost.device_id, &key(2), "", 1099, LIMITS),
        Err(DeviceError::CooldownActive {
            retry_after_secs: 1
        })
    ));
    assert!(matches!(
        recovery_finish(&mut conn, 1, &lost.device_id, &key(3), "", 1100, LIMITS),
        Err(DeviceError::RecoveryUnknown)
    ));
    assert!(matches!(
        recovery_finish(&mut conn, 2, &lost.device_id, &key(2), "", 1100, LIMITS),
        Err(DeviceError::RecoveryUnknown)
    ));
    let done =
        recovery_finish(&mut conn, 1, &lost.device_id, &key(2), "New", 1100, LIMITS).unwrap();
    assert_eq!(done.number, lost.number);
    assert_eq!(
        devices::by_id(&conn, &lost.device_id)
            .unwrap()
            .unwrap()
            .state,
        DeviceState::Replaced
    );
    assert!(matches!(
        recovery_finish(&mut conn, 1, &lost.device_id, &key(2), "", 1200, LIMITS),
        Err(DeviceError::RecoveryUnknown)
    ));
}

#[test]
fn recovery_can_be_cancelled_expires_and_dies_with_the_device() {
    let db = db();
    let mut conn = db.lock();
    let lost = register(&mut conn, 1, &key(1), "", 0, LIMITS).unwrap();
    assert!(matches!(
        recovery_cancel(&mut conn, 1, 0),
        Err(DeviceError::RecoveryUnknown)
    ));
    recovery_start(&mut conn, 1, &lost.device_id, &key(2), 0, COOLDOWN).unwrap();
    recovery_cancel(&mut conn, 1, 10).unwrap();
    assert!(matches!(
        recovery_finish(&mut conn, 1, &lost.device_id, &key(2), "", 5000, LIMITS),
        Err(DeviceError::RecoveryUnknown)
    ));
    // Expiry: 7 days after the cool-down.
    recovery_start(&mut conn, 1, &lost.device_id, &key(2), 0, COOLDOWN).unwrap();
    let late = COOLDOWN + RECOVERY_WINDOW_SECS;
    assert!(matches!(
        recovery_finish(&mut conn, 1, &lost.device_id, &key(2), "", late, LIMITS),
        Err(DeviceError::RecoveryUnknown)
    ));
    // Removing the "lost" device (it was found) drops the recovery.
    recovery_start(&mut conn, 1, &lost.device_id, &key(2), late, COOLDOWN).unwrap();
    assert!(recovery_clear(&conn, 1).unwrap());
    assert!(!recovery_clear(&conn, 1).unwrap());
    recovery_start(&mut conn, 1, &lost.device_id, &key(2), late, COOLDOWN).unwrap();
    remove(&mut conn, 1, &lost.device_id, late).unwrap();
    assert!(matches!(
        recovery_finish(
            &mut conn,
            1,
            &lost.device_id,
            &key(2),
            "",
            late + COOLDOWN,
            LIMITS
        ),
        Err(DeviceError::RecoveryUnknown)
    ));
}

#[test]
fn recovery_of_a_legacy_device_rebinds_it_and_foreign_devices_are_unknown() {
    let db = db();
    let mut conn = db.lock();
    assert!(matches!(
        recovery_start(&mut conn, 2, "legacy:old-1", &key(2), 0, COOLDOWN),
        Err(DeviceError::Unknown)
    ));
    recovery_start(&mut conn, 1, "legacy:old-1", &key(2), 0, COOLDOWN).unwrap();
    let done =
        recovery_finish(&mut conn, 1, "legacy:old-1", &key(2), "", COOLDOWN, LIMITS).unwrap();
    assert_eq!(
        (done.device_row, done.number.as_str()),
        (50, "UOS-AAAA-0001")
    );
    let taken = register(&mut conn, 2, &key(3), "", 0, LIMITS).unwrap();
    assert!(matches!(
        recovery_start(&mut conn, 1, &done.device_id, &key(3), 0, COOLDOWN),
        Err(DeviceError::Taken)
    ));
    assert!(!taken.number.is_empty());
}
