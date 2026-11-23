//! The only ways a number comes to another key: upgrade of a legacy registration, replacement
//! authorised by the old device, and replacement of a lost device after password check, cool-down
//! and notice. The HTTP layer verifies every signature before it calls these functions; each
//! function is one database transaction.

use crate::device_events::{self, EventKind};
use crate::devices::{
    self, allocate_number, charge_quota, clean_name, day_of, ensure_unused, in_use, insert_active,
    number_of, DeviceError, DeviceRow, DeviceState, Limits, Registered, Standing,
};
use crate::identity::{self, LEGACY_PREFIX, PUBLIC_KEY_BYTES};
use rusqlite::{Connection, OptionalExtension, Transaction};
use subtle::ConstantTimeEq;

/// How long a recovery can be finished once its cool-down is over (7 days).
pub const RECOVERY_WINDOW_SECS: i64 = 7 * 86_400;

/// The time of the migration to device keys: account tokens issued later cannot upgrade a legacy
/// registration.
pub fn legacy_cutoff(conn: &Connection) -> rusqlite::Result<i64> {
    let text: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key='legacy_cutoff'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    // No recorded time: no token qualifies (fail closed).
    Ok(text.and_then(|t| t.parse().ok()).unwrap_or(i64::MIN))
}

/// The new key of a move: the key itself, its name and when it gets management rights.
struct NewKey<'a> {
    public_key: &'a [u8; PUBLIC_KEY_BYTES],
    name: &'a str,
    standing: Standing,
}

fn manage_in(standing: Standing, now: i64, limits: Limits) -> i64 {
    if standing.approved {
        return 0;
    }
    standing
        .key_since
        .saturating_add(limits.cooldown_secs)
        .saturating_sub(now)
        .max(0)
}

/// Binds an existing row to the new key in place; the number stays with the row.
fn rebind(
    tx: &Transaction<'_>,
    old: &DeviceRow,
    new: &NewKey<'_>,
    now: i64,
    limits: Limits,
) -> Result<Registered, DeviceError> {
    let (public_key, name) = (new.public_key, new.name);
    let device_id = identity::device_id(public_key);
    tx.execute(
        "UPDATE devices SET device_id=?1, public_key=?2, state=1, name=?3, epoch=epoch+1,
                            last_seen=?4, approved=?5, key_since=?6
         WHERE id=?7",
        rusqlite::params![
            device_id,
            public_key.as_slice(),
            name,
            day_of(now),
            i64::from(new.standing.approved),
            new.standing.key_since,
            old.row
        ],
    )?;
    let number = match number_of(tx, old.row)? {
        Some(n) => n,
        None => allocate_number(tx, old.account_id, old.row, now)?,
    };
    Ok(Registered {
        device_row: old.row,
        device_id,
        number,
        epoch: old.epoch + 1,
        created: true,
        manage_in: manage_in(new.standing, now, limits),
    })
}

/// Moves the number of `old` to the new key: a legacy row is rebound in place, an active device
/// is marked replaced and a new row takes its number.
fn transfer(
    tx: &Transaction<'_>,
    old: &DeviceRow,
    new: &NewKey<'_>,
    now: i64,
    limits: Limits,
) -> Result<Registered, DeviceError> {
    let (public_key, name) = (new.public_key, new.name);
    let device_id = identity::device_id(public_key);
    ensure_unused(tx, &device_id)?;
    charge_quota(tx, old.account_id, limits)?;
    tx.execute("DELETE FROM device_recoveries WHERE old_row=?1", [old.row])?;
    if old.state == DeviceState::Legacy {
        return rebind(tx, old, new, now, limits);
    }
    let number = number_of(tx, old.row)?;
    tx.execute(
        "UPDATE devices SET state=3, epoch=epoch+1 WHERE id=?1",
        [old.row],
    )?;
    let device_row = insert_active(tx, old.account_id, public_key, name, now, new.standing)?;
    let number = match number {
        Some(n) => {
            tx.execute(
                "UPDATE numbers SET device_row=?1 WHERE number=?2",
                rusqlite::params![device_row, n],
            )?;
            n
        }
        None => allocate_number(tx, old.account_id, device_row, now)?,
    };
    Ok(Registered {
        device_row,
        device_id,
        number,
        epoch: 0,
        created: true,
        manage_in: manage_in(new.standing, now, limits),
    })
}

/// Who asks for the upgrade of which legacy registration.
#[derive(Debug, Clone, Copy)]
pub struct LegacyClaim<'a> {
    /// Account of the presented account token.
    pub account_id: i64,
    /// The id the device used before device keys (without the `legacy:` prefix).
    pub legacy_id: &'a str,
    /// Issue time of the presented account token.
    pub token_issued: i64,
}

/// Gives a legacy registration its key. Allowed only for the owning account and only with an
/// account token issued before the migration; everything else is
/// [`DeviceError::LegacyNotEligible`], without telling which condition failed.
pub fn upgrade_legacy(
    conn: &mut Connection,
    claim: LegacyClaim<'_>,
    public_key: &[u8; PUBLIC_KEY_BYTES],
    name: &str,
    now: i64,
    limits: Limits,
) -> Result<Registered, DeviceError> {
    let name = clean_name(name)?;
    let account_id = claim.account_id;
    if claim.legacy_id.len() > devices::MAX_DEVICE_ID_BYTES - LEGACY_PREFIX.len() {
        return Err(DeviceError::LegacyNotEligible);
    }
    let tx = conn.transaction()?;
    let old = devices::by_id(&tx, &format!("{LEGACY_PREFIX}{}", claim.legacy_id))?
        .filter(|d| d.account_id == account_id && d.state == DeviceState::Legacy)
        .ok_or(DeviceError::LegacyNotEligible)?;
    // Strictly before: a token issued in the very second of the migration may already come from
    // the migrated server.
    if claim.token_issued >= legacy_cutoff(&tx)? {
        return Err(DeviceError::LegacyNotEligible);
    }
    // Like a registration: at once only when no other device of the account is in use.
    let others: i64 = tx.query_row(
        "SELECT COUNT(*) FROM devices WHERE account_id=?1 AND state IN (0,1) AND id<>?2",
        rusqlite::params![account_id, old.row],
        |r| r.get(0),
    )?;
    let new = NewKey {
        public_key,
        name,
        standing: Standing {
            approved: others == 0,
            key_since: now,
        },
    };
    let done = transfer(&tx, &old, &new, now, limits)?;
    device_events::push(
        &tx,
        account_id,
        EventKind::DeviceUpgraded,
        &done.device_id,
        now,
    )?;
    tx.commit()?;
    Ok(done)
}

/// Moves the number of the active device `old` to `new_key`. The caller verified the signature of
/// the old key over this very replacement and the proof of possession of the new key. The new
/// device stands where the old one stood: it manages the account's devices from the moment the
/// old one did (or would have).
pub fn replace(
    conn: &mut Connection,
    old: &DeviceRow,
    new_key: &[u8; PUBLIC_KEY_BYTES],
    name: &str,
    now: i64,
    limits: Limits,
) -> Result<Registered, DeviceError> {
    let name = clean_name(name)?;
    let tx = conn.transaction()?;
    // Read again inside the transaction: the row must still be the active device the caller saw.
    let current = in_use(&tx, old.account_id, &old.device_id)?;
    if current.state != DeviceState::Active || current.row != old.row {
        return Err(DeviceError::Unknown);
    }
    let new = NewKey {
        public_key: new_key,
        name,
        standing: Standing {
            approved: current.approved,
            key_since: current.key_since,
        },
    };
    let done = transfer(&tx, &current, &new, now, limits)?;
    device_events::push(
        &tx,
        old.account_id,
        EventKind::DeviceReplaced,
        &done.device_id,
        now,
    )?;
    tx.commit()?;
    Ok(done)
}

/// A registration from before device keys has no key and so no way to be told about a recovery
/// or to cancel it. While account tokens from before the migration are still valid (30 days),
/// its own device can take it over with [`upgrade_legacy`]; a recovery – password only – is not
/// offered during that time.
fn legacy_recovery_open(conn: &Connection, now: i64) -> Result<(), DeviceError> {
    // No recorded migration time: never open (fail closed).
    let opens = match legacy_cutoff(conn)? {
        i64::MIN => i64::MAX,
        cutoff => cutoff.saturating_add(crate::accounts::TOKEN_TTL_SECS),
    };
    if now < opens {
        return Err(DeviceError::RecoveryLocked {
            retry_after_secs: u64::try_from(opens - now).unwrap_or(u64::MAX),
        });
    }
    Ok(())
}

/// A recovery that is pending for an account, as it is reported to the account's devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryInfo {
    /// Id of the device whose number is about to move.
    pub old_device: String,
    /// Seconds until the recovery can be finished (0 = it can).
    pub eligible_in: i64,
}

/// The pending recovery of an account, if any.
pub fn recovery_info(
    conn: &Connection,
    account_id: i64,
    now: i64,
) -> rusqlite::Result<Option<RecoveryInfo>> {
    conn.query_row(
        "SELECT d.device_id, r.eligible_at FROM device_recoveries r
         JOIN devices d ON d.id = r.old_row
         WHERE r.account_id=?1 AND r.expires_at>?2",
        rusqlite::params![account_id, now],
        |r| {
            Ok(RecoveryInfo {
                old_device: r.get(0)?,
                eligible_in: r.get::<_, i64>(1)?.saturating_sub(now).max(0),
            })
        },
    )
    .optional()
}

struct Pending {
    old_row: i64,
    new_key: Vec<u8>,
    eligible_at: i64,
    expires_at: i64,
}

fn pending(conn: &Connection, account_id: i64, now: i64) -> rusqlite::Result<Option<Pending>> {
    conn.execute(
        "DELETE FROM device_recoveries WHERE account_id=?1 AND expires_at<=?2",
        rusqlite::params![account_id, now],
    )?;
    conn.query_row(
        "SELECT old_row, new_key, eligible_at, expires_at FROM device_recoveries WHERE account_id=?1",
        [account_id],
        |r| {
            Ok(Pending {
                old_row: r.get(0)?,
                new_key: r.get(1)?,
                eligible_at: r.get(2)?,
                expires_at: r.get(3)?,
            })
        },
    )
    .optional()
}

fn same_request(p: &Pending, old_row: i64, new_key: &[u8; PUBLIC_KEY_BYTES]) -> bool {
    p.old_row == old_row && bool::from(p.new_key.as_slice().ct_eq(new_key.as_slice()))
}

/// Starts the replacement of a lost device (`old_device_id`) by `new_key`. The caller verified the
/// account password and the proof of possession of the new key. Returns the time from which the
/// recovery can be finished. Repeating the same request does not restart the cool-down.
pub fn recovery_start(
    conn: &mut Connection,
    account_id: i64,
    old_device_id: &str,
    new_key: &[u8; PUBLIC_KEY_BYTES],
    now: i64,
    cooldown_secs: i64,
) -> Result<i64, DeviceError> {
    let tx = conn.transaction()?;
    let old = in_use(&tx, account_id, old_device_id)?;
    ensure_unused(&tx, &identity::device_id(new_key))?;
    if old.state == DeviceState::Legacy {
        legacy_recovery_open(&tx, now)?;
    }
    if let Some(p) = pending(&tx, account_id, now)? {
        if same_request(&p, old.row, new_key) {
            tx.commit()?;
            return Ok(p.eligible_at);
        }
        return Err(DeviceError::RecoveryPending);
    }
    let eligible_at = now.saturating_add(cooldown_secs.max(0));
    tx.execute(
        "INSERT INTO device_recoveries(account_id, old_row, new_key, started, eligible_at, expires_at)
         VALUES(?1,?2,?3,?4,?5,?6)",
        rusqlite::params![
            account_id,
            old.row,
            new_key.as_slice(),
            now,
            eligible_at,
            eligible_at.saturating_add(RECOVERY_WINDOW_SECS)
        ],
    )?;
    device_events::push(
        &tx,
        account_id,
        EventKind::RecoveryStarted,
        old_device_id,
        now,
    )?;
    tx.commit()?;
    Ok(eligible_at)
}

/// Cancels the pending recovery of the account.
pub fn recovery_cancel(
    conn: &mut Connection,
    account_id: i64,
    now: i64,
) -> Result<(), DeviceError> {
    let tx = conn.transaction()?;
    if pending(&tx, account_id, now)?.is_none() {
        // Commit the removal of an expired row, then report.
        tx.commit()?;
        return Err(DeviceError::RecoveryUnknown);
    }
    tx.execute(
        "DELETE FROM device_recoveries WHERE account_id=?1",
        [account_id],
    )?;
    device_events::push(&tx, account_id, EventKind::RecoveryCancelled, "", now)?;
    tx.commit()?;
    Ok(())
}

/// Drops a pending recovery without notice of its own (password change). True when there was one.
pub fn recovery_clear(conn: &Connection, account_id: i64) -> rusqlite::Result<bool> {
    let n = conn.execute(
        "DELETE FROM device_recoveries WHERE account_id=?1",
        [account_id],
    )?;
    Ok(n > 0)
}

/// Finishes a recovery after its cool-down: the number of the lost device moves to `new_key`,
/// which must be the key the recovery was started with.
pub fn recovery_finish(
    conn: &mut Connection,
    account_id: i64,
    old_device_id: &str,
    new_key: &[u8; PUBLIC_KEY_BYTES],
    name: &str,
    now: i64,
    limits: Limits,
) -> Result<Registered, DeviceError> {
    let name = clean_name(name)?;
    let tx = conn.transaction()?;
    let Some(p) = pending(&tx, account_id, now)? else {
        tx.commit()?;
        return Err(DeviceError::RecoveryUnknown);
    };
    let old = in_use(&tx, account_id, old_device_id).map_err(|e| match e {
        DeviceError::Unknown => DeviceError::RecoveryUnknown,
        other => other,
    })?;
    if !same_request(&p, old.row, new_key) || now >= p.expires_at {
        return Err(DeviceError::RecoveryUnknown);
    }
    if now < p.eligible_at {
        return Err(DeviceError::CooldownActive {
            retry_after_secs: u64::try_from(p.eligible_at - now).unwrap_or(u64::MAX),
        });
    }
    // Password, cool-down and notice are behind this key: it manages the account's devices.
    let new = NewKey {
        public_key: new_key,
        name,
        standing: Standing {
            approved: true,
            key_since: now,
        },
    };
    let done = transfer(&tx, &old, &new, now, limits)?;
    device_events::push(
        &tx,
        account_id,
        EventKind::RecoveryFinished,
        &done.device_id,
        now,
    )?;
    tx.commit()?;
    Ok(done)
}

#[cfg(test)]
#[path = "device_moves_tests.rs"]
mod tests;
