//! Security notices of an account: what happened to its devices. Devices read them with a device
//! token; connected signalling sockets get them pushed. Only the newest [`MAX_EVENTS`] are kept.

use crate::identity;
use rusqlite::Connection;

/// Notices kept per account.
pub const MAX_EVENTS: i64 = 64;
/// Characters of a device id shown in a notice.
pub const NOTICE_PREFIX_LEN: usize = 8;

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// A device key was registered.
    DeviceAdded,
    /// A device was removed.
    DeviceRemoved,
    /// A number moved to a new device, authorised by the old one.
    DeviceReplaced,
    /// A registration from before device keys got its key.
    DeviceUpgraded,
    /// Replacement of a lost device was requested; the cool-down runs.
    RecoveryStarted,
    /// A pending recovery was cancelled.
    RecoveryCancelled,
    /// A recovery was completed; the number moved.
    RecoveryFinished,
    /// The account password was changed.
    PasswordChanged,
    /// A managing device gave an added device its management rights.
    DeviceApproved,
}

impl EventKind {
    /// The stable name used in the API and the database.
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::DeviceAdded => "device_added",
            EventKind::DeviceRemoved => "device_removed",
            EventKind::DeviceReplaced => "device_replaced",
            EventKind::DeviceUpgraded => "device_upgraded",
            EventKind::RecoveryStarted => "recovery_started",
            EventKind::RecoveryCancelled => "recovery_cancelled",
            EventKind::RecoveryFinished => "recovery_finished",
            EventKind::PasswordChanged => "password_changed",
            EventKind::DeviceApproved => "device_approved",
        }
    }
}

/// One stored notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Increasing id; pass the highest seen id as `after` to read on.
    pub id: i64,
    /// Event name ([`EventKind::as_str`]).
    pub kind: String,
    /// First characters of the device id concerned (empty for account events).
    pub device: String,
    /// Time (Unix seconds).
    pub at: i64,
}

/// Appends a notice and drops all but the newest [`MAX_EVENTS`] of the account. The notice of a
/// recovery that is still pending is never dropped, however many notices follow it: whoever
/// started the recovery cannot push it out of the list.
pub fn push(
    conn: &Connection,
    account_id: i64,
    kind: EventKind,
    device_id: &str,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO device_events(account_id, ts, kind, device) VALUES(?1,?2,?3,?4)",
        rusqlite::params![
            account_id,
            now,
            kind.as_str(),
            identity::id_prefix(device_id, NOTICE_PREFIX_LEN)
        ],
    )?;
    conn.execute(
        "DELETE FROM device_events WHERE account_id=?1 AND id NOT IN
           (SELECT id FROM device_events WHERE account_id=?1 ORDER BY id DESC LIMIT ?2)
           AND NOT (kind='recovery_started'
                    AND EXISTS (SELECT 1 FROM device_recoveries r WHERE r.account_id=?1)
                    AND id = (SELECT MAX(id) FROM device_events
                              WHERE account_id=?1 AND kind='recovery_started'))",
        rusqlite::params![account_id, MAX_EVENTS],
    )?;
    Ok(())
}

/// The notices of an account with an id above `after`, oldest first.
pub fn after(conn: &Connection, account_id: i64, after: i64) -> rusqlite::Result<Vec<Event>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, kind, device, ts FROM device_events
         WHERE account_id=?1 AND id>?2 ORDER BY id LIMIT ?3",
    )?;
    // One more than the cap: the protected notice of a pending recovery may be kept on top.
    let rows = stmt.query_map(rusqlite::params![account_id, after, MAX_EVENTS + 1], |r| {
        Ok(Event {
            id: r.get(0)?,
            kind: r.get(1)?,
            device: r.get(2)?,
            at: r.get(3)?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn notices_are_kept_per_account_capped_and_carry_only_a_prefix() {
        let db = Db::open(":memory:").unwrap();
        let conn = db.lock();
        conn.execute_batch(
            "INSERT INTO accounts(id, username, pw_hash, created) VALUES(1,'a','h',0),(2,'b','h',0);",
        )
        .unwrap();
        let id = "abcdefghijklmnopqrstuvwxyz234567";
        for i in 0..(MAX_EVENTS + 10) {
            push(&conn, 1, EventKind::DeviceAdded, id, i).unwrap();
        }
        push(&conn, 2, EventKind::PasswordChanged, "", 5).unwrap();
        let mine = after(&conn, 1, 0).unwrap();
        assert_eq!(mine.len() as i64, MAX_EVENTS);
        assert_eq!(mine[0].at, 10, "the oldest ten were dropped");
        assert_eq!(mine[0].device, "abcdefgh");
        assert_eq!(mine[0].kind, "device_added");
        let last = mine.last().unwrap().id;
        assert!(after(&conn, 1, last).unwrap().is_empty());
        let other = after(&conn, 2, 0).unwrap();
        assert_eq!((other.len(), other[0].device.as_str()), (1, ""));
    }

    #[test]
    fn the_notice_of_a_pending_recovery_survives_any_number_of_later_notices() {
        let db = Db::open(":memory:").unwrap();
        let conn = db.lock();
        conn.execute_batch(
            "INSERT INTO accounts(id, username, pw_hash, created) VALUES(1,'a','h',0);
             INSERT INTO device_recoveries VALUES(1, 5, x'00', 0, 10, 20);",
        )
        .unwrap();
        push(&conn, 1, EventKind::RecoveryStarted, "lostdevice", 1).unwrap();
        for i in 0..(3 * MAX_EVENTS) {
            push(&conn, 1, EventKind::DeviceAdded, "throwaway", 2 + i).unwrap();
        }
        let list = after(&conn, 1, 0).unwrap();
        assert_eq!(list.len() as i64, MAX_EVENTS + 1);
        assert_eq!(list[0].kind, "recovery_started");
        // Once the recovery is over, the notice ages out like any other.
        conn.execute("DELETE FROM device_recoveries", []).unwrap();
        push(&conn, 1, EventKind::DeviceAdded, "throwaway", 999).unwrap();
        let list = after(&conn, 1, 0).unwrap();
        assert_eq!(list.len() as i64, MAX_EVENTS);
        assert!(list.iter().all(|e| e.kind != "recovery_started"));
    }

    #[test]
    fn event_names_are_distinct() {
        let all = [
            EventKind::DeviceAdded,
            EventKind::DeviceRemoved,
            EventKind::DeviceReplaced,
            EventKind::DeviceUpgraded,
            EventKind::RecoveryStarted,
            EventKind::RecoveryCancelled,
            EventKind::RecoveryFinished,
            EventKind::PasswordChanged,
            EventKind::DeviceApproved,
        ];
        let names: std::collections::HashSet<_> = all.iter().map(|k| k.as_str()).collect();
        assert_eq!(names.len(), all.len());
    }
}
