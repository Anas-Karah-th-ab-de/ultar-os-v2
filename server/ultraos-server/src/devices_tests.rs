use super::*;
use crate::db::Db;
use ed25519_dalek::SigningKey;

const COOLDOWN: i64 = 1000;
const LIMITS: Limits = Limits {
    max_devices: 3,
    cooldown_secs: COOLDOWN,
};

fn db() -> Db {
    let db = Db::open(":memory:").unwrap();
    db.lock()
        .execute_batch(
            "INSERT INTO accounts(id, username, pw_hash, created) VALUES(1,'a','h',0),(2,'b','h',0);",
        )
        .unwrap();
    db
}

fn key(seed: u8) -> [u8; 32] {
    SigningKey::from_bytes(&[seed; 32])
        .verifying_key()
        .to_bytes()
}

#[test]
fn registration_assigns_one_number_per_key_and_is_idempotent() {
    let db = db();
    let mut conn = db.lock();
    let first = register(&mut conn, 1, &key(1), " Phone ", 1000, LIMITS).unwrap();
    assert!(first.created && first.epoch == 0);
    assert_eq!(first.device_id, identity::device_id(&key(1)));
    assert_eq!(first.number.len(), 13);
    let again = register(&mut conn, 1, &key(1), "Other name", 2000, LIMITS).unwrap();
    assert!(!again.created);
    assert_eq!(
        (again.number, again.device_row),
        (first.number.clone(), first.device_row)
    );
    let second = register(&mut conn, 1, &key(2), "", 1000, LIMITS).unwrap();
    assert_ne!(second.number, first.number);
    let listed = list(&conn, 1, 0, COOLDOWN).unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(
        listed[0].name, "Phone",
        "trimmed, and not changed by the repeat"
    );
    assert_eq!(listed[0].number.as_deref(), Some(first.number.as_str()));
    assert_eq!(listed[0].state, DeviceState::Active);
}

#[test]
fn a_key_belongs_to_one_account() {
    let db = db();
    let mut conn = db.lock();
    register(&mut conn, 1, &key(1), "", 0, LIMITS).unwrap();
    assert!(matches!(
        register(&mut conn, 2, &key(1), "", 0, LIMITS),
        Err(DeviceError::Taken)
    ));
    assert!(list(&conn, 2, 0, COOLDOWN).unwrap().is_empty());
    let quota: i64 = conn
        .query_row(
            "SELECT device_quota_used FROM accounts WHERE id=2",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(quota, 0, "a refused registration costs nothing");
}

#[test]
fn device_limit_counts_devices_in_use_only() {
    let db = db();
    let mut conn = db.lock();
    for seed in 1..=3 {
        register(&mut conn, 1, &key(seed), "", 0, LIMITS).unwrap();
    }
    assert!(matches!(
        register(&mut conn, 1, &key(4), "", 0, LIMITS),
        Err(DeviceError::Limit)
    ));
    assert!(
        register(&mut conn, 2, &key(4), "", 0, LIMITS).is_ok(),
        "per account"
    );
    remove(&mut conn, 1, &identity::device_id(&key(1)), 5).unwrap();
    assert!(register(&mut conn, 1, &key(5), "", 0, LIMITS).is_ok());
}

#[test]
fn lifetime_quota_stops_register_remove_loops() {
    let db = db();
    let mut conn = db.lock();
    let limits = Limits {
        max_devices: 1,
        cooldown_secs: COOLDOWN,
    };
    assert_eq!(limits.quota(), 10);
    for seed in 1..=10u8 {
        let done = register(&mut conn, 1, &key(seed), "", 0, limits).unwrap();
        remove(&mut conn, 1, &done.device_id, 1).unwrap();
    }
    assert!(matches!(
        register(&mut conn, 1, &key(11), "", 0, limits),
        Err(DeviceError::QuotaExhausted)
    ));
    let (numbers, retired): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), SUM(state=0) FROM numbers WHERE account_id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (numbers, retired),
        (10, 10),
        "bounded, and every number stays retired"
    );
}

#[test]
fn removal_retires_key_and_number_for_good() {
    let db = db();
    let mut conn = db.lock();
    let done = register(&mut conn, 1, &key(1), "", 0, LIMITS).unwrap();
    assert_eq!(
        remove(&mut conn, 1, &done.device_id, 9).unwrap().as_deref(),
        Some(done.number.as_str())
    );
    assert!(matches!(
        remove(&mut conn, 1, &done.device_id, 9),
        Err(DeviceError::Unknown)
    ));
    for account in [1, 2] {
        assert!(matches!(
            register(&mut conn, account, &key(1), "", 10, LIMITS),
            Err(DeviceError::Retired)
        ));
    }
    let row = by_id(&conn, &done.device_id).unwrap().unwrap();
    assert_eq!((row.state, row.epoch), (DeviceState::Removed, 1));
    assert_eq!(number_of(&conn, row.row).unwrap(), None);
    assert!(list(&conn, 1, 0, COOLDOWN).unwrap().is_empty());
    // Another account's device cannot be removed or renamed.
    let other = register(&mut conn, 2, &key(2), "", 0, LIMITS).unwrap();
    assert!(matches!(
        remove(&mut conn, 1, &other.device_id, 0),
        Err(DeviceError::Unknown)
    ));
    assert!(matches!(
        rename(&conn, 1, &other.device_id, "x"),
        Err(DeviceError::Unknown)
    ));
}

#[test]
fn a_retired_number_is_never_issued_again() {
    let db = db();
    let conn = db.lock();
    conn.execute_batch(
        "INSERT INTO devices(id, account_id, device_id, state, created) VALUES(7,1,'x',1,0);",
    )
    .unwrap();
    // Every number the generator can produce is retired or taken → nothing is handed out.
    for _ in 0..50 {
        let n = allocate_number(&conn, 1, 7, 0).unwrap();
        conn.execute("DELETE FROM numbers WHERE number=?1", [&n])
            .unwrap();
        let retired: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM retired_numbers WHERE number=?1",
                [&n],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            retired, 1,
            "the trigger keeps deleted numbers out of circulation"
        );
    }
    let n = gen_number();
    assert_eq!(n.len(), 13);
    assert!(n.starts_with("UOS-") && n.as_bytes()[8] == b'-');
    assert!(n[4..8]
        .bytes()
        .chain(n[9..].bytes())
        .all(|b| b.is_ascii_hexdigit()));
}

#[test]
fn names_are_validated_and_renaming_works() {
    let db = db();
    let mut conn = db.lock();
    let done = register(&mut conn, 1, &key(1), "", 0, LIMITS).unwrap();
    rename(&conn, 1, &done.device_id, "  هاتف أنس  ").unwrap();
    assert_eq!(list(&conn, 1, 0, COOLDOWN).unwrap()[0].name, "هاتف أنس");
    let long = "x".repeat(MAX_NAME_CHARS + 1);
    for bad in ["a\nb", "tab\there", long.as_str(), "nul\0"] {
        assert!(matches!(
            rename(&conn, 1, &done.device_id, bad),
            Err(DeviceError::InvalidName)
        ));
        assert!(matches!(
            register(&mut conn, 1, &key(9), bad, 0, LIMITS),
            Err(DeviceError::InvalidName)
        ));
    }
    assert!(
        clean_name(&"ي".repeat(MAX_NAME_CHARS)).is_ok(),
        "characters, not bytes"
    );
}

#[test]
fn last_seen_is_stored_as_a_day_and_written_once_per_day() {
    let db = db();
    let mut conn = db.lock();
    let t0 = 1_800_000_000 + 4321;
    let done = register(&mut conn, 1, &key(1), "", t0, LIMITS).unwrap();
    let seen = |conn: &Connection| list(conn, 1, 0, COOLDOWN).unwrap()[0].last_seen;
    assert_eq!(seen(&conn), day_of(t0));
    assert_eq!(seen(&conn) % DAY_SECS, 0);
    touch(&conn, done.device_row, t0 + 3600).unwrap();
    assert_eq!(conn.changes(), 0, "same day: no write");
    touch(&conn, done.device_row, t0 + DAY_SECS).unwrap();
    assert_eq!(seen(&conn), day_of(t0) + DAY_SECS);
    touch(&conn, done.device_row, t0).unwrap();
    assert_eq!(seen(&conn), day_of(t0) + DAY_SECS, "never moves back");
    assert_eq!(list(&conn, 1, 0, COOLDOWN).unwrap()[0].added % DAY_SECS, 0);
    assert_eq!(day_of(-1), -DAY_SECS);
}

#[test]
fn legacy_rows_are_readable_but_not_creatable() {
    let db = db();
    let conn = db.lock();
    conn.execute_batch(
        "INSERT INTO devices(id, account_id, device_id, state, created) VALUES(5,1,'legacy:old-1',0,0);
         INSERT INTO numbers(number, account_id, device_row, state, created) VALUES('UOS-AAAA-0001',1,5,1,0);",
    )
    .unwrap();
    assert_eq!(
        legacy_number(&conn, 1, "old-1").unwrap().as_deref(),
        Some("UOS-AAAA-0001")
    );
    assert_eq!(
        legacy_number(&conn, 2, "old-1").unwrap(),
        None,
        "other account"
    );
    assert_eq!(legacy_number(&conn, 1, "unknown").unwrap(), None);
    assert_eq!(legacy_number(&conn, 1, &"x".repeat(500)).unwrap(), None);
    let row = in_use(&conn, 1, "legacy:old-1").unwrap();
    assert_eq!((row.state, row.public_key), (DeviceState::Legacy, None));
    assert!(matches!(
        in_use(&conn, 1, &"y".repeat(500)),
        Err(DeviceError::Unknown)
    ));
    assert_eq!(account_epoch(&conn, 1).unwrap(), Some(0));
    assert_eq!(account_epoch(&conn, 99).unwrap(), None);
}

#[test]
fn states_round_trip_and_unknown_codes_fail_closed() {
    for s in [
        DeviceState::Legacy,
        DeviceState::Active,
        DeviceState::Removed,
        DeviceState::Replaced,
    ] {
        assert_eq!(DeviceState::from_code(s.code()), s);
        assert!(!s.as_str().is_empty());
    }
    assert_eq!(DeviceState::from_code(77), DeviceState::Removed);
    assert!(DeviceState::Legacy.in_use() && DeviceState::Active.in_use());
    assert!(!DeviceState::Removed.in_use() && !DeviceState::Replaced.in_use());
    let e = DeviceError::from(rusqlite::Error::QueryReturnedNoRows);
    assert!(std::error::Error::source(&e).is_some());
    assert!(DeviceError::CooldownActive {
        retry_after_secs: 5
    }
    .to_string()
    .contains('5'));
    assert!(std::error::Error::source(&DeviceError::Limit).is_none());
}

#[test]
fn only_the_first_device_manages_at_once_others_need_approval_or_time() {
    let db = db();
    let mut conn = db.lock();
    let first = register(&mut conn, 1, &key(1), "", 0, LIMITS).unwrap();
    let second = register(&mut conn, 1, &key(2), "", 100, LIMITS).unwrap();
    assert_eq!((first.manage_in, second.manage_in), (0, COOLDOWN));
    let row = |conn: &Connection, id: &str| by_id(conn, id).unwrap().unwrap();
    let added = row(&conn, &second.device_id);
    assert_eq!(added.manage_in(600, COOLDOWN), Some(500));
    assert!(matches!(
        added.require_manager(600, COOLDOWN),
        Err(DeviceError::NotApproved {
            retry_after_secs: 500
        })
    ));
    assert!(
        added.require_manager(1100, COOLDOWN).is_ok(),
        "after the cool-down"
    );
    // Registering again does not shorten or restart the wait.
    let again = register(&mut conn, 1, &key(2), "", 600, LIMITS).unwrap();
    assert_eq!(again.manage_in, 500);
    let listed = list(&conn, 1, 600, COOLDOWN).unwrap();
    assert_eq!(
        (listed[0].manage_in, listed[1].manage_in),
        (Some(0), Some(500))
    );
    // An unapproved device cannot approve; a managing one can, and only inside its account.
    let third = register(&mut conn, 1, &key(3), "", 100, LIMITS).unwrap();
    assert!(matches!(
        approve(&mut conn, &added, &third.device_id, 600, COOLDOWN),
        Err(DeviceError::NotApproved { .. })
    ));
    let manager = row(&conn, &first.device_id);
    let foreign = register(&mut conn, 2, &key(9), "", 0, LIMITS).unwrap();
    assert!(matches!(
        approve(&mut conn, &manager, &foreign.device_id, 600, COOLDOWN),
        Err(DeviceError::Unknown)
    ));
    approve(&mut conn, &manager, &second.device_id, 600, COOLDOWN).unwrap();
    assert_eq!(
        row(&conn, &second.device_id).manage_in(600, COOLDOWN),
        Some(0)
    );
    // A removed device manages nothing, and a removed approver approves nothing.
    remove(&mut conn, 1, &first.device_id, 700).unwrap();
    assert_eq!(row(&conn, &first.device_id).manage_in(700, COOLDOWN), None);
    assert!(matches!(
        approve(&mut conn, &manager, &third.device_id, 700, COOLDOWN),
        Err(DeviceError::Unknown)
    ));
    assert_eq!(by_row(&conn, 9999).unwrap(), None);
}
