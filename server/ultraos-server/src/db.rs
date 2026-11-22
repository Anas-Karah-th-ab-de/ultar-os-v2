//! SQLite storage (rusqlite, bundled). Opening a database always brings it to the current schema.
use rusqlite::Connection;
use std::fmt;
use std::sync::{Mutex, MutexGuard};

/// The database handle: one connection behind a mutex.
pub struct Db(pub Mutex<Connection>);

/// Errors when opening or migrating the database.
#[derive(Debug)]
pub enum DbError {
    /// SQLite reported an error.
    Sqlite(rusqlite::Error),
    /// The file was written by a newer server; refusing to touch it.
    NewerSchema {
        /// Schema version found in the file.
        found: i64,
        /// Highest schema version this build understands.
        supported: i64,
    },
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::Sqlite(e) => write!(f, "sqlite: {e}"),
            DbError::NewerSchema { found, supported } => {
                write!(
                    f,
                    "database schema {found} is newer than supported {supported}"
                )
            }
        }
    }
}

impl std::error::Error for DbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DbError::Sqlite(e) => Some(e),
            DbError::NewerSchema { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::Sqlite(e)
    }
}

impl Db {
    /// Opens (or creates) the database file and migrates it in place to the current schema.
    pub fn open(path: &str) -> Result<Self, DbError> {
        let mut c = Connection::open(path)?;
        c.pragma_update(None, "journal_mode", "WAL")?;
        c.pragma_update(None, "foreign_keys", "ON")?;
        c.busy_timeout(std::time::Duration::from_secs(5))?;
        crate::migrate::run(&mut c)?;
        Ok(Db(Mutex::new(c)))
    }

    /// Locks the connection. A poisoned lock is recovered: SQLite keeps its own consistency.
    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Schema of the first released server (schema version 0). Kept verbatim: fresh databases start
/// here and are then migrated exactly like an existing installation.
pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS accounts (
    id         INTEGER PRIMARY KEY,
    username   TEXT UNIQUE NOT NULL,
    pw_hash    TEXT NOT NULL,          -- Argon2id (PHC-String)
    created    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS devices (
    id         INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    device_id  TEXT UNIQUE NOT NULL,   -- Hash(Seriennr + Gerätepubkey)
    pubkey     TEXT NOT NULL,          -- ed25519 (base64) des Geräts
    created    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS numbers (
    number     TEXT PRIMARY KEY,       -- UOS-XXXX-XXXX
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    device_id  TEXT NOT NULL REFERENCES devices(device_id) ON DELETE CASCADE,
    created    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS wg_peers (
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    pubkey     TEXT NOT NULL,          -- WireGuard-Pubkey des Geräts
    address    TEXT NOT NULL,          -- zugewiesene interne IP (10.80.x.y/32)
    created    INTEGER NOT NULL,
    PRIMARY KEY (account_id, pubkey)
);
CREATE TABLE IF NOT EXISTS store_apps (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    version    TEXT NOT NULL,
    size       INTEGER NOT NULL,
    sha256     TEXT NOT NULL,
    sig        TEXT NOT NULL,          -- ed25519-Signatur des Pakets (base64)
    url        TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS releases (
    channel    TEXT NOT NULL,          -- stable | beta
    version    TEXT NOT NULL,
    url        TEXT NOT NULL,
    roothash   TEXT NOT NULL,          -- dm-verity Root-Hash des Images
    sig        TEXT NOT NULL,          -- ed25519-Signatur (base64)
    created    INTEGER NOT NULL,
    PRIMARY KEY (channel, version)
);
CREATE TABLE IF NOT EXISTS audit (
    id      INTEGER PRIMARY KEY,
    ts      INTEGER NOT NULL,
    who     TEXT,
    action  TEXT NOT NULL,
    detail  TEXT
);
"#;

/// Current wall-clock time as Unix seconds (0 if the clock is before 1970).
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Appends one row to the audit table (administrative actions).
pub fn audit(conn: &Connection, who: &str, action: &str, detail: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO audit(ts, who, action, detail) VALUES(?1,?2,?3,?4)",
        rusqlite::params![now(), who, action, detail],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_creates_current_schema_and_audit_works() {
        let db = Db::open(":memory:").unwrap();
        let conn = db.lock();
        audit(&conn, "admin", "test", "detail").unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, crate::migrate::LATEST);
    }

    #[test]
    fn errors_display_and_chain() {
        let e = DbError::NewerSchema {
            found: 9,
            supported: 1,
        };
        assert!(e.to_string().contains('9'));
        assert!(std::error::Error::source(&e).is_none());
        let e = DbError::from(rusqlite::Error::QueryReturnedNoRows);
        assert!(std::error::Error::source(&e).is_some());
        assert!(e.to_string().starts_with("sqlite"));
    }

    #[test]
    fn now_is_after_2020() {
        assert!(now() > 1_577_836_800);
    }
}
