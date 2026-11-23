//! In-place schema migrations, tracked with SQLite's `PRAGMA user_version`.
//!
//! Version 0 is the schema of the first released server (see [`crate::db::SCHEMA`]); a database
//! written by that server has `user_version = 0` and is upgraded here without data loss. Each
//! migration runs in one transaction together with the version bump, so an interrupted upgrade
//! leaves the old schema intact.
//!
//! Version 1 (store and updates without an account, entitlements):
//! * `store_apps` gains description, category, publisher, premium, permissions, installed size
//!   and a normalised search text;
//! * new tables `store_versions` (version history), `store_filetypes`, `publishers`,
//!   `update_channels`, `entitlements`;
//! * `releases` is rebuilt with a numeric id, architecture and edition.
//!
//! Version 2 (device identity, see `docs/DEVICE-IDENTITY.md` §9): [`crate::migrate_v2`].
//! Version 3 (approval of added devices): `devices.approved`, `devices.key_since`.

use crate::db::DbError;
use crate::search;
use rusqlite::{Connection, Transaction};

/// The schema version this build reads and writes.
pub const LATEST: i64 = 3;

/// Brings the database to [`LATEST`]. Idempotent.
pub fn run(conn: &mut Connection) -> Result<(), DbError> {
    let found: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if found > LATEST {
        return Err(DbError::NewerSchema {
            found,
            supported: LATEST,
        });
    }
    let tx = conn.transaction()?;
    tx.execute_batch(crate::db::SCHEMA)?;
    tx.execute_batch(crate::verbund::SCHEMA)?;
    if found < 1 {
        to_v1(&tx)?;
    }
    if found < 2 {
        crate::migrate_v2::apply(&tx, crate::db::now())?;
    }
    if found < 3 {
        crate::migrate_v2::apply_v3(&tx)?;
    }
    if found < LATEST {
        tx.pragma_update(None, "user_version", LATEST)?;
        tracing::info!(event = "db_migrated", from = found, to = LATEST);
    }
    tx.commit()?;
    Ok(())
}

pub(crate) fn column_exists(
    tx: &Transaction<'_>,
    table: &str,
    column: &str,
) -> rusqlite::Result<bool> {
    let n: i64 = tx.query_row(
        "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2",
        rusqlite::params![table, column],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Adds a column unless it is already there. `table`, `column` and `decl` are compile-time
/// constants of this module, never external input.
pub(crate) fn add_column(
    tx: &Transaction<'_>,
    table: &str,
    column: &str,
    decl: &str,
) -> rusqlite::Result<()> {
    if !column_exists(tx, table, column)? {
        tx.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl};"))?;
    }
    Ok(())
}

const STORE_APP_COLUMNS: [(&str, &str); 8] = [
    ("description", "TEXT NOT NULL DEFAULT ''"),
    ("category", "TEXT NOT NULL DEFAULT 'other'"),
    ("publisher", "TEXT NOT NULL DEFAULT ''"),
    ("premium", "INTEGER NOT NULL DEFAULT 0"),
    ("permissions", "TEXT NOT NULL DEFAULT '[]'"),
    ("installed_size", "INTEGER NOT NULL DEFAULT 0"),
    ("search_text", "TEXT NOT NULL DEFAULT ''"),
    ("stored", "INTEGER NOT NULL DEFAULT 0"),
];

const V1_TABLES: &str = r#"
CREATE TABLE IF NOT EXISTS store_versions (
    app_id     TEXT NOT NULL REFERENCES store_apps(id) ON DELETE CASCADE,
    version    TEXT NOT NULL,
    size       INTEGER NOT NULL,
    sha256     TEXT NOT NULL,
    sig        TEXT NOT NULL DEFAULT '',
    url        TEXT NOT NULL DEFAULT '',   -- external location of rows from schema 0
    stored     INTEGER NOT NULL DEFAULT 0, -- 1 = file is in the server's file store
    changelog  TEXT NOT NULL DEFAULT '',
    created    INTEGER NOT NULL,
    PRIMARY KEY (app_id, version)
);
CREATE TABLE IF NOT EXISTS store_filetypes (
    app_id     TEXT NOT NULL REFERENCES store_apps(id) ON DELETE CASCADE,
    ext        TEXT NOT NULL,              -- lower case, without dot
    PRIMARY KEY (app_id, ext)
);
CREATE INDEX IF NOT EXISTS store_filetypes_ext ON store_filetypes(ext);
CREATE INDEX IF NOT EXISTS store_apps_category ON store_apps(category);
CREATE TABLE IF NOT EXISTS publishers (
    name       TEXT PRIMARY KEY,
    verified   INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS update_channels (
    name       TEXT PRIMARY KEY,
    gated      INTEGER NOT NULL DEFAULT 0  -- 1 = needs token + entitlement
);
CREATE TABLE IF NOT EXISTS entitlements (
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('app','channel')),
    item       TEXT NOT NULL,
    granted    INTEGER NOT NULL,
    PRIMARY KEY (account_id, kind, item)
);
INSERT OR IGNORE INTO update_channels(name, gated) VALUES
    ('stable', 0), ('beta', 0), ('preview', 1), ('lts', 1);
"#;

const V1_RELEASES: &str = r#"
CREATE TABLE releases_v1 (
    id         INTEGER PRIMARY KEY,
    channel    TEXT NOT NULL,
    version    TEXT NOT NULL,
    arch       TEXT NOT NULL DEFAULT 'any',
    edition    TEXT NOT NULL DEFAULT 'any',
    url        TEXT NOT NULL DEFAULT '',   -- external location of rows from schema 0
    roothash   TEXT NOT NULL,
    sig        TEXT NOT NULL,
    size       INTEGER NOT NULL DEFAULT 0,
    sha256     TEXT NOT NULL DEFAULT '',
    stored     INTEGER NOT NULL DEFAULT 0,
    created    INTEGER NOT NULL,
    UNIQUE (channel, version, arch, edition)
);
INSERT INTO releases_v1(channel, version, url, roothash, sig, created)
    SELECT channel, version, url, roothash, sig, created FROM releases ORDER BY created, version;
DROP TABLE releases;
ALTER TABLE releases_v1 RENAME TO releases;
CREATE INDEX releases_lookup ON releases(channel, created);
-- Channels that already carried releases were readable by every account; they stay open.
INSERT OR IGNORE INTO update_channels(name, gated) SELECT DISTINCT channel, 0 FROM releases;
"#;

fn to_v1(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    for (column, decl) in STORE_APP_COLUMNS {
        add_column(tx, "store_apps", column, decl)?;
    }
    tx.execute_batch(V1_TABLES)?;
    tx.execute(
        "INSERT OR IGNORE INTO store_versions(app_id, version, size, sha256, sig, url, stored, created)
         SELECT id, version, size, sha256, sig, url, 0, 0 FROM store_apps",
        [],
    )?;
    if !column_exists(tx, "releases", "id")? {
        tx.execute_batch(V1_RELEASES)?;
    }
    backfill_search_text(tx)
}

fn backfill_search_text(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    let rows: Vec<(String, String, String)> = {
        let mut stmt = tx.prepare("SELECT id, name, description FROM store_apps")?;
        let mapped = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        mapped.collect::<rusqlite::Result<_>>()?
    };
    for (id, name, description) in rows {
        tx.execute(
            "UPDATE store_apps SET search_text = ?1 WHERE id = ?2",
            rusqlite::params![search::search_text(&name, &description), id],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn old_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(crate::db::SCHEMA).unwrap();
        c.execute_batch(crate::verbund::SCHEMA).unwrap();
        c
    }

    #[test]
    fn fresh_database_reaches_latest_and_rerun_is_a_no_op() {
        let mut c = Connection::open_in_memory().unwrap();
        run(&mut c).unwrap();
        run(&mut c).unwrap();
        let v: i64 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, LATEST);
        let gated: i64 = c
            .query_row(
                "SELECT gated FROM update_channels WHERE name='preview'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(gated, 1);
    }

    #[test]
    fn old_rows_survive_and_get_defaults() {
        let mut c = old_db();
        c.execute_batch(
            "INSERT INTO store_apps VALUES('a.b','Übung مَكتَب','1.0',5,'aa','s','/x.uap');
             INSERT INTO releases VALUES('stable','1','u1','r1','s1',10);
             INSERT INTO releases VALUES('stable','2','u2','r2','s2',20);
             INSERT INTO releases VALUES('nightly','3','u3','r3','s3',30);",
        )
        .unwrap();
        run(&mut c).unwrap();
        let (premium, search, cat): (i64, String, String) = c
            .query_row(
                "SELECT premium, search_text, category FROM store_apps",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((premium, cat.as_str()), (0, "other"));
        assert!(
            search.contains("übung") && search.contains("مكتب"),
            "{search}"
        );
        let (url, stored): (String, i64) = c
            .query_row(
                "SELECT url, stored FROM store_versions WHERE app_id='a.b'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((url.as_str(), stored), ("/x.uap", 0));
        let ids: Vec<(i64, String, String)> = c
            .prepare("SELECT id, version, arch FROM releases ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(ids.len(), 3);
        assert_eq!(ids[0], (1, "1".to_string(), "any".to_string()));
        let nightly: i64 = c
            .query_row(
                "SELECT gated FROM update_channels WHERE name='nightly'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(nightly, 0, "a channel that already had releases stays open");
    }

    #[test]
    fn newer_database_is_refused_untouched() {
        let mut c = old_db();
        c.pragma_update(None, "user_version", LATEST + 1).unwrap();
        let err = run(&mut c).unwrap_err();
        assert!(matches!(err, DbError::NewerSchema { found, .. } if found == LATEST + 1));
        assert!(!column_exists(&c.transaction().unwrap(), "store_apps", "premium").unwrap());
    }

    #[test]
    fn half_applied_columns_do_not_break_the_migration() {
        let mut c = old_db();
        c.execute_batch("ALTER TABLE store_apps ADD COLUMN premium INTEGER NOT NULL DEFAULT 0;")
            .unwrap();
        run(&mut c).unwrap();
        let tx = c.transaction().unwrap();
        assert!(column_exists(&tx, "store_apps", "search_text").unwrap());
        assert!(column_exists(&tx, "releases", "edition").unwrap());
    }
}
