//! Schema version 2: device identity (`docs/DEVICE-IDENTITY.md` §7 and §9).
//!
//! * `devices` is rebuilt: public key as 32 bytes (NULL for rows from before device keys), state,
//!   name, token epoch, last-seen day. Existing rows become legacy rows; their id moves into the
//!   `legacy:` namespace and the free text that was stored as "pubkey" is dropped.
//! * `numbers` is rebuilt to point at the device row and gains a state (active / retired).
//! * `retired_numbers` and a trigger keep every number that ever existed out of circulation, also
//!   when its row disappears with a deleted account.
//! * `accounts` gains the token epoch and the lifetime registration counter.
//! * New: `device_recoveries`, `device_events`, `meta` (time of this migration).

use crate::migrate::{add_column, column_exists};
use rusqlite::Transaction;

const REBUILD: &str = r#"
CREATE TABLE devices_v2 (
    id         INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    device_id  TEXT UNIQUE NOT NULL,       -- base32 key id, or 'legacy:<id of schema 1>'
    public_key BLOB,                       -- Ed25519, 32 bytes; NULL = legacy row without key
    state      INTEGER NOT NULL,           -- 0 legacy, 1 active, 2 removed, 3 replaced
    name       TEXT NOT NULL DEFAULT '',
    epoch      INTEGER NOT NULL DEFAULT 0, -- device tokens carry it; raised on removal/replacement
    created    INTEGER NOT NULL,
    last_seen  INTEGER NOT NULL DEFAULT 0  -- start of the day of the last contact, 0 = unknown
);
INSERT INTO devices_v2(id, account_id, device_id, public_key, state, name, epoch, created, last_seen)
    SELECT id, account_id, 'legacy:' || device_id, NULL, 0, '', 0, created, 0 FROM devices;
CREATE TABLE numbers_v2 (
    number     TEXT PRIMARY KEY,           -- UOS-XXXX-XXXX
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    device_row INTEGER NOT NULL REFERENCES devices_v2(id) ON DELETE CASCADE,
    state      INTEGER NOT NULL DEFAULT 1, -- 1 active, 0 retired
    created    INTEGER NOT NULL
);
INSERT INTO numbers_v2(number, account_id, device_row, state, created)
    SELECT n.number, n.account_id, d.id, 1, n.created
    FROM numbers n JOIN devices d ON d.device_id = n.device_id;
INSERT OR IGNORE INTO retired_numbers(number)
    SELECT number FROM numbers WHERE number NOT IN (SELECT number FROM numbers_v2);
DROP TABLE numbers;
DROP TABLE devices;
ALTER TABLE devices_v2 RENAME TO devices;
ALTER TABLE numbers_v2 RENAME TO numbers;
CREATE INDEX devices_account ON devices(account_id, state);
CREATE INDEX numbers_device ON numbers(device_row);
"#;

const TABLES: &str = r#"
CREATE TABLE IF NOT EXISTS retired_numbers (
    number     TEXT PRIMARY KEY
);
CREATE TABLE IF NOT EXISTS device_recoveries (
    account_id  INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    old_row     INTEGER NOT NULL,          -- devices.id of the lost device
    new_key     BLOB NOT NULL,             -- public key that will take over
    started     INTEGER NOT NULL,
    eligible_at INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS device_events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    ts         INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    device     TEXT NOT NULL DEFAULT ''    -- first characters of the device id
);
CREATE INDEX IF NOT EXISTS device_events_account ON device_events(account_id, id);
CREATE TABLE IF NOT EXISTS meta (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL
);
"#;

const TRIGGER: &str = r#"
CREATE TRIGGER IF NOT EXISTS numbers_retire AFTER DELETE ON numbers
BEGIN
    INSERT OR IGNORE INTO retired_numbers(number) VALUES (old.number);
END;
"#;

/// Applies schema version 2 inside the migration transaction. `now` becomes the legacy cut-off:
/// only account tokens issued up to this moment may upgrade a legacy registration.
pub(crate) fn apply(tx: &Transaction<'_>, now: i64) -> rusqlite::Result<()> {
    add_column(tx, "accounts", "token_epoch", "INTEGER NOT NULL DEFAULT 0")?;
    add_column(
        tx,
        "accounts",
        "device_quota_used",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    tx.execute_batch(TABLES)?;
    if !column_exists(tx, "devices", "public_key")? {
        tx.execute_batch(REBUILD)?;
    }
    tx.execute_batch(TRIGGER)?;
    tx.execute(
        "INSERT OR IGNORE INTO meta(key, value) VALUES('legacy_cutoff', ?1)",
        [now.to_string()],
    )?;
    Ok(())
}

/// Schema version 3: a device added to an account that already has devices manages other
/// devices only after approval or after the cool-down since its key was bound.
///
/// * `devices.approved` – 1 when the device may manage other devices at once. Rows that exist
///   when this migration runs keep their rights (default 1).
/// * `devices.key_since` – time the current key was bound (0 for rows from before).
pub(crate) fn apply_v3(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    add_column(tx, "devices", "approved", "INTEGER NOT NULL DEFAULT 1")?;
    add_column(tx, "devices", "key_since", "INTEGER NOT NULL DEFAULT 0")
}

#[cfg(test)]
mod tests {
    use crate::migrate::{run, LATEST};
    use rusqlite::Connection;

    type LegacyRow = (i64, i64, String, Option<Vec<u8>>, i64, i64);

    /// A database with two devices and their numbers in the shape of schema 1, migrated by the
    /// real migration.
    fn v1_db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c.execute_batch(crate::db::SCHEMA).unwrap();
        c.execute_batch(
            "INSERT INTO accounts(id, username, pw_hash, created) VALUES(1,'a','h',5),(2,'b','h',6);
             INSERT INTO devices VALUES(1,1,'aaaa','cHVi',10);
             INSERT INTO devices VALUES(2,2,'bbbb','',11);
             INSERT INTO numbers VALUES('UOS-0001-0001',1,'aaaa',10);
             INSERT INTO numbers VALUES('UOS-0002-0002',2,'bbbb',11);",
        )
        .unwrap();
        run(&mut c).unwrap();
        c
    }

    fn fresh_v2() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        run(&mut c).unwrap();
        c
    }

    #[test]
    fn existing_devices_become_legacy_rows_and_keep_their_numbers() {
        let c = v1_db();
        let rows: Vec<LegacyRow> = c
            .prepare("SELECT id, account_id, device_id, public_key, state, epoch FROM devices ORDER BY id")
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            rows,
            vec![
                (1, 1, "legacy:aaaa".to_string(), None, 0, 0),
                (2, 2, "legacy:bbbb".to_string(), None, 0, 0)
            ]
        );
        let numbers: Vec<(String, i64, i64, i64)> = c
            .prepare("SELECT number, account_id, device_row, state FROM numbers ORDER BY number")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            numbers,
            vec![
                ("UOS-0001-0001".to_string(), 1, 1, 1),
                ("UOS-0002-0002".to_string(), 2, 2, 1)
            ]
        );
        let version: i64 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, LATEST);
        let cutoff: String = c
            .query_row(
                "SELECT value FROM meta WHERE key='legacy_cutoff'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(cutoff.parse::<i64>().unwrap() > 1_577_836_800);
    }

    #[test]
    fn running_again_changes_nothing() {
        let mut c = v1_db();
        let cutoff = |c: &Connection| -> String {
            c.query_row(
                "SELECT value FROM meta WHERE key='legacy_cutoff'",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        c.execute("UPDATE meta SET value='123' WHERE key='legacy_cutoff'", [])
            .unwrap();
        run(&mut c).unwrap();
        // Even a forced second application keeps rows and the recorded cut-off.
        let tx = c.transaction().unwrap();
        super::apply(&tx, 999).unwrap();
        tx.commit().unwrap();
        assert_eq!(cutoff(&c), "123");
        let n: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM devices WHERE device_id LIKE 'legacy:legacy:%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "ids are not prefixed twice");
    }

    #[test]
    fn deleted_numbers_are_retired_also_through_account_deletion() {
        let c = v1_db();
        c.execute("DELETE FROM accounts WHERE id=1", []).unwrap();
        let left: i64 = c
            .query_row("SELECT COUNT(*) FROM devices", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 1, "devices go with their account");
        let retired: Vec<String> = c
            .prepare("SELECT number FROM retired_numbers")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(retired, vec!["UOS-0001-0001".to_string()]);
    }

    #[test]
    fn a_fresh_database_gets_the_same_shape() {
        let c = fresh_v2();
        for (table, column) in [
            ("devices", "public_key"),
            ("devices", "last_seen"),
            ("devices", "approved"),
            ("devices", "key_since"),
            ("numbers", "device_row"),
            ("accounts", "token_epoch"),
            ("accounts", "device_quota_used"),
            ("device_recoveries", "eligible_at"),
            ("device_events", "kind"),
        ] {
            let n: i64 = c
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name=?2",
                    [table, column],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "{table}.{column}");
        }
        let old: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('devices') WHERE name='pubkey'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old, 0, "the free-text key column is gone");
    }
}
