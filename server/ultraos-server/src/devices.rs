//! Devices of an account: registration of a device key, the Ultar number, listing, renaming and
//! removal. All functions work on a database connection and return typed errors; the HTTP layer
//! ([`crate::device_api`]) verifies proofs of possession before it calls them.
//!
//! Invariants kept here (see `docs/DEVICE-IDENTITY.md` §6):
//! * a device id belongs to exactly one public key and one account, for good;
//! * a removed or replaced key is never registered again;
//! * a number is never given to a different key, except by the explicit moves in
//!   [`crate::device_moves`]; a retired number is never issued again.

pub use crate::device_error::DeviceError;
use crate::device_events::{self, EventKind};
use crate::identity::{self, LEGACY_PREFIX, PUBLIC_KEY_BYTES};
use rand::Rng;
use rusqlite::{Connection, OptionalExtension};
use subtle::ConstantTimeEq;

/// Seconds per day; last-seen and creation times leave the server rounded down to this.
pub const DAY_SECS: i64 = 86_400;
/// Longest device name in characters.
pub const MAX_NAME_CHARS: usize = 64;
/// Longest device id accepted from a client (legacy ids were up to 128 bytes).
pub const MAX_DEVICE_ID_BYTES: usize = 140;
/// A registered key may be bound this many times the device limit over an account's lifetime.
pub const QUOTA_FACTOR: i64 = 10;

const NUMBER_ATTEMPTS: usize = 20;

/// Lifecycle state of a device row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// Registered before device keys existed; has no key yet.
    Legacy,
    /// Registered with a key.
    Active,
    /// Removed by its account; the key is retired.
    Removed,
    /// Its number moved to another key; the key is retired.
    Replaced,
}

impl DeviceState {
    /// The value stored in the database.
    pub fn code(self) -> i64 {
        match self {
            DeviceState::Legacy => 0,
            DeviceState::Active => 1,
            DeviceState::Removed => 2,
            DeviceState::Replaced => 3,
        }
    }

    /// Parses a stored value; unknown values count as removed (fail closed).
    pub fn from_code(code: i64) -> Self {
        match code {
            0 => DeviceState::Legacy,
            1 => DeviceState::Active,
            3 => DeviceState::Replaced,
            _ => DeviceState::Removed,
        }
    }

    /// The name used in the API.
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceState::Legacy => "legacy",
            DeviceState::Active => "active",
            DeviceState::Removed => "removed",
            DeviceState::Replaced => "replaced",
        }
    }

    /// True for devices that are in use (legacy or active).
    pub fn in_use(self) -> bool {
        matches!(self, DeviceState::Legacy | DeviceState::Active)
    }
}

/// Limits of one account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Most devices in use at the same time.
    pub max_devices: i64,
}

impl Limits {
    /// Key registrations allowed over the account's lifetime.
    pub fn quota(self) -> i64 {
        self.max_devices.saturating_mul(QUOTA_FACTOR)
    }
}

/// Result of a registration, upgrade or move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    /// Row of the device.
    pub device_row: i64,
    /// Key id of the device.
    pub device_id: String,
    /// Its Ultar number.
    pub number: String,
    /// Token epoch of the device.
    pub epoch: i64,
    /// False when the device was registered already and nothing changed.
    pub created: bool,
}

/// A device row as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRow {
    /// Row id.
    pub row: i64,
    /// Owning account.
    pub account_id: i64,
    /// Key id, or `legacy:<old id>`.
    pub device_id: String,
    /// Public key (`None` for legacy rows).
    pub public_key: Option<[u8; PUBLIC_KEY_BYTES]>,
    /// Lifecycle state.
    pub state: DeviceState,
    /// Token epoch.
    pub epoch: i64,
}

/// One entry of the device list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Key id, or `legacy:<old id>`.
    pub device_id: String,
    /// Name given by the user.
    pub name: String,
    /// Number, if the device has one.
    pub number: Option<String>,
    /// Lifecycle state.
    pub state: DeviceState,
    /// Day of registration (Unix seconds, start of day).
    pub added: i64,
    /// Day of last contact (0 = never since keys exist).
    pub last_seen: i64,
}

/// Rounds a time down to the start of its day.
pub fn day_of(now: i64) -> i64 {
    now - now.rem_euclid(DAY_SECS)
}

/// Trims and validates a device name.
pub fn clean_name(name: &str) -> Result<&str, DeviceError> {
    let name = name.trim();
    if name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err(DeviceError::InvalidName);
    }
    Ok(name)
}

/// A random number of the form `UOS-XXXX-XXXX`.
pub fn gen_number() -> String {
    let mut r = rand::thread_rng();
    format!("UOS-{:04X}-{:04X}", r.gen::<u16>(), r.gen::<u16>())
}

fn read_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DeviceRow> {
    let key: Option<Vec<u8>> = r.get(3)?;
    Ok(DeviceRow {
        row: r.get(0)?,
        account_id: r.get(1)?,
        device_id: r.get(2)?,
        public_key: key.and_then(|k| <[u8; PUBLIC_KEY_BYTES]>::try_from(k.as_slice()).ok()),
        state: DeviceState::from_code(r.get(4)?),
        epoch: r.get(5)?,
    })
}

/// The device with this id, in any state.
pub fn by_id(conn: &Connection, device_id: &str) -> rusqlite::Result<Option<DeviceRow>> {
    conn.query_row(
        "SELECT id, account_id, device_id, public_key, state, epoch FROM devices WHERE device_id=?1",
        [device_id],
        read_row,
    )
    .optional()
}

/// The device of `account_id` with this id, if it is in use.
pub fn in_use(
    conn: &Connection,
    account_id: i64,
    device_id: &str,
) -> Result<DeviceRow, DeviceError> {
    if device_id.len() > MAX_DEVICE_ID_BYTES {
        return Err(DeviceError::Unknown);
    }
    by_id(conn, device_id)?
        .filter(|d| d.account_id == account_id && d.state.in_use())
        .ok_or(DeviceError::Unknown)
}

/// The active number of a device row.
pub fn number_of(conn: &Connection, device_row: i64) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT number FROM numbers WHERE device_row=?1 AND state=1",
        [device_row],
        |r| r.get(0),
    )
    .optional()
}

/// The token epoch of an account (`None` when the account does not exist).
pub fn account_epoch(conn: &Connection, account_id: i64) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT token_epoch FROM accounts WHERE id=?1",
        [account_id],
        |r| r.get(0),
    )
    .optional()
}

/// Fails unless `id` is free: an existing row means the key is taken or retired.
pub(crate) fn ensure_unused(conn: &Connection, id: &str) -> Result<(), DeviceError> {
    match by_id(conn, id)? {
        None => Ok(()),
        Some(d) if d.state.in_use() => Err(DeviceError::Taken),
        Some(_) => Err(DeviceError::Retired),
    }
}

pub(crate) fn charge_quota(
    conn: &Connection,
    account_id: i64,
    limits: Limits,
) -> Result<(), DeviceError> {
    let used: i64 = conn.query_row(
        "SELECT device_quota_used FROM accounts WHERE id=?1",
        [account_id],
        |r| r.get(0),
    )?;
    if used >= limits.quota() {
        return Err(DeviceError::QuotaExhausted);
    }
    conn.execute(
        "UPDATE accounts SET device_quota_used = device_quota_used + 1 WHERE id=?1",
        [account_id],
    )?;
    Ok(())
}

fn devices_in_use(conn: &Connection, account_id: i64) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM devices WHERE account_id=?1 AND state IN (0,1)",
        [account_id],
        |r| r.get(0),
    )
}

pub(crate) fn allocate_number(
    conn: &Connection,
    account_id: i64,
    device_row: i64,
    now: i64,
) -> Result<String, DeviceError> {
    for _ in 0..NUMBER_ATTEMPTS {
        let n = gen_number();
        let retired: Option<i64> = conn
            .query_row("SELECT 1 FROM retired_numbers WHERE number=?1", [&n], |r| {
                r.get(0)
            })
            .optional()?;
        if retired.is_some() {
            continue;
        }
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO numbers(number, account_id, device_row, state, created)
             VALUES(?1,?2,?3,1,?4)",
            rusqlite::params![n, account_id, device_row, now],
        )?;
        if inserted == 1 {
            return Ok(n);
        }
    }
    Err(DeviceError::NumbersExhausted)
}

/// Inserts an active device row for `public_key` (the caller checked that its id is unused).
pub(crate) fn insert_active(
    conn: &Connection,
    account_id: i64,
    public_key: &[u8; PUBLIC_KEY_BYTES],
    name: &str,
    now: i64,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO devices(account_id, device_id, public_key, state, name, epoch, created, last_seen)
         VALUES(?1,?2,?3,1,?4,0,?5,?6)",
        rusqlite::params![
            account_id,
            identity::device_id(public_key),
            public_key.as_slice(),
            name,
            now,
            day_of(now)
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Registers `public_key` for `account_id` and assigns its number. The caller has verified that
/// the requester holds the private key. Registering an active key of the same account again
/// returns the existing number.
pub fn register(
    conn: &mut Connection,
    account_id: i64,
    public_key: &[u8; PUBLIC_KEY_BYTES],
    name: &str,
    now: i64,
    limits: Limits,
) -> Result<Registered, DeviceError> {
    let name = clean_name(name)?;
    let device_id = identity::device_id(public_key);
    let tx = conn.transaction()?;
    if let Some(found) = by_id(&tx, &device_id)? {
        let same_key = found
            .public_key
            .is_some_and(|k| bool::from(k.ct_eq(public_key)));
        if !found.state.in_use() {
            return Err(DeviceError::Retired);
        }
        if found.account_id != account_id || !same_key {
            return Err(DeviceError::Taken);
        }
        let number = match number_of(&tx, found.row)? {
            Some(n) => n,
            None => allocate_number(&tx, account_id, found.row, now)?,
        };
        tx.commit()?;
        return Ok(Registered {
            device_row: found.row,
            device_id,
            number,
            epoch: found.epoch,
            created: false,
        });
    }
    if devices_in_use(&tx, account_id)? >= limits.max_devices {
        return Err(DeviceError::Limit);
    }
    charge_quota(&tx, account_id, limits)?;
    let device_row = insert_active(&tx, account_id, public_key, name, now)?;
    let number = allocate_number(&tx, account_id, device_row, now)?;
    device_events::push(&tx, account_id, EventKind::DeviceAdded, &device_id, now)?;
    tx.commit()?;
    Ok(Registered {
        device_row,
        device_id,
        number,
        epoch: 0,
        created: true,
    })
}

/// The number of a legacy registration of `account_id` (read only; for clients of the first
/// release that still send `device_id` + `pubkey`).
pub fn legacy_number(
    conn: &Connection,
    account_id: i64,
    legacy_id: &str,
) -> Result<Option<String>, DeviceError> {
    if legacy_id.len() > MAX_DEVICE_ID_BYTES - LEGACY_PREFIX.len() {
        return Ok(None);
    }
    let found = by_id(conn, &format!("{LEGACY_PREFIX}{legacy_id}"))?
        .filter(|d| d.account_id == account_id && d.state == DeviceState::Legacy);
    match found {
        Some(d) => Ok(number_of(conn, d.row)?),
        None => Ok(None),
    }
}

/// The devices of an account that are in use, oldest first.
pub fn list(conn: &Connection, account_id: i64) -> rusqlite::Result<Vec<DeviceInfo>> {
    let mut stmt = conn.prepare_cached(
        "SELECT d.device_id, d.name, n.number, d.state, d.created, d.last_seen
         FROM devices d LEFT JOIN numbers n ON n.device_row = d.id AND n.state = 1
         WHERE d.account_id=?1 AND d.state IN (0,1) ORDER BY d.id LIMIT 1000",
    )?;
    let rows = stmt.query_map([account_id], |r| {
        Ok(DeviceInfo {
            device_id: r.get(0)?,
            name: r.get(1)?,
            number: r.get(2)?,
            state: DeviceState::from_code(r.get(3)?),
            added: day_of(r.get(4)?),
            last_seen: r.get(5)?,
        })
    })?;
    rows.collect()
}

/// Renames a device of the account.
pub fn rename(
    conn: &Connection,
    account_id: i64,
    device_id: &str,
    name: &str,
) -> Result<(), DeviceError> {
    let name = clean_name(name)?;
    let device = in_use(conn, account_id, device_id)?;
    conn.execute(
        "UPDATE devices SET name=?1 WHERE id=?2",
        rusqlite::params![name, device.row],
    )?;
    Ok(())
}

/// Removes a device of the account: its key and its number are retired, its token epoch moves on
/// (all its tokens are dead) and a pending recovery that names it is dropped. Returns the retired
/// number so that the caller can close the device's signalling socket.
pub fn remove(
    conn: &mut Connection,
    account_id: i64,
    device_id: &str,
    now: i64,
) -> Result<Option<String>, DeviceError> {
    let tx = conn.transaction()?;
    let device = in_use(&tx, account_id, device_id)?;
    let number = number_of(&tx, device.row)?;
    tx.execute(
        "UPDATE devices SET state=2, epoch=epoch+1 WHERE id=?1",
        [device.row],
    )?;
    tx.execute(
        "UPDATE numbers SET state=0 WHERE device_row=?1",
        [device.row],
    )?;
    tx.execute(
        "DELETE FROM device_recoveries WHERE old_row=?1",
        [device.row],
    )?;
    device_events::push(&tx, account_id, EventKind::DeviceRemoved, device_id, now)?;
    tx.commit()?;
    Ok(number)
}

/// Records contact of a device: the day only, written at most once per day.
pub fn touch(conn: &Connection, device_row: i64, now: i64) -> rusqlite::Result<()> {
    let day = day_of(now);
    conn.execute(
        "UPDATE devices SET last_seen=?1 WHERE id=?2 AND last_seen<?1",
        rusqlite::params![day, device_row],
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "devices_tests.rs"]
mod tests;
