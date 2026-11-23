//! The only ways a number comes to another key: upgrade of a legacy registration, replacement
//! authorised by the old device, and replacement of a lost device after password check, cool-down
//! and notice. The HTTP layer verifies every signature before it calls these functions; each
//! function is one database transaction.

use crate::device_events::{self, EventKind};
use crate::devices::{
    self, allocate_number, charge_quota, clean_name, day_of, ensure_unused, in_use, insert_active,
    number_of, DeviceError, DeviceRow, DeviceState, Limits, Registered,
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

/// Binds an existing row to `public_key` in place; the number stays with the row.
fn rebind(
    tx: &Transaction<'_>,
    old: &DeviceRow,
    public_key: &[u8; PUBLIC_KEY_BYTES],
    name: &str,
    now: i64,
) -> Result<Registered, DeviceError> {
    let device_id = identity::device_id(public_key);
    tx.execute(
        "UPDATE devices SET device_id=?1, public_key=?2, state=1, name=?3, epoch=epoch+1, last_seen=?4
         WHERE id=?5",
        rusqlite::params![device_id, public_key.as_slice(), name, day_of(now), old.row],
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
    })
}

/// Moves the number of `old` to `public_key`: a legacy row is rebound in place, an active device
/// is marked replaced and a new row takes its number.
fn transfer(
    tx: &Transaction<'_>,
    old: &DeviceRow,
    public_key: &[u8; PUBLIC_KEY_BYTES],
    name: &str,
    now: i64,
    limits: Limits,
) -> Result<Registered, DeviceError> {
    let device_id = identity::device_id(public_key);
    ensure_unused(tx, &device_id)?;
    charge_quota(tx, old.account_id, limits)?;
    tx.execute("DELETE FROM device_recoveries WHERE old_row=?1", [old.row])?;
    if old.state == DeviceState::Legacy {
        return rebind(tx, old, public_key, name, now);
    }
    let number = number_of(tx, old.row)?;
    tx.execute(
        "UPDATE devices SET state=3, epoch=epoch+1 WHERE id=?1",
        [old.row],
    )?;
    let device_row = insert_active(tx, old.account_id, public_key, name, now)?;
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
    let done = transfer(&tx, &old, public_key, name, now, limits)?;
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
/// the old key over this very replacement and the proof of possession of the new key.
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
    let done = transfer(&tx, &current, new_key, name, now, limits)?;
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
    let done = transfer(&tx, &old, new_key, name, now, limits)?;
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
