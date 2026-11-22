//! SQLite-Datenhaltung (rusqlite, gebündelt). Für Produktion leicht auf Postgres umstellbar.
use rusqlite::Connection;
use std::sync::Mutex;

pub struct Db(pub Mutex<Connection>);

impl Db {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let c = Connection::open(path)?;
        c.pragma_update(None, "journal_mode", "WAL")?;
        c.pragma_update(None, "foreign_keys", "ON")?;
        c.execute_batch(SCHEMA)?;
        c.execute_batch(crate::verbund::SCHEMA)?;
        Ok(Db(Mutex::new(c)))
    }
}

const SCHEMA: &str = r#"
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

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
