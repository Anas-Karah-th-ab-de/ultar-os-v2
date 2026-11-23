//! Session checks and entitlements.
//!
//! The store catalogue, free apps and the free update channels need no account. Premium apps and
//! gated update channels need a valid session token **and** an entitlement record for that
//! account. Records are granted and revoked by an administrator (see [`crate::admin`]); there is
//! no payment logic here.
//!
//! Tokens are verified by recomputing an HMAC-SHA256 and comparing it in constant time
//! (`hmac::Mac::verify_slice`); see [`crate::auth::verify_token`].

use crate::error::ApiError;
use crate::{auth, db, AppState};
use axum::http::HeaderMap;
use rusqlite::Connection;

/// What an entitlement unlocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntitlementKind {
    /// A premium app (item = app id).
    App,
    /// A gated update channel (item = channel name).
    Channel,
}

impl EntitlementKind {
    /// The value stored in the database and used on the command line.
    pub fn as_str(self) -> &'static str {
        match self {
            EntitlementKind::App => "app",
            EntitlementKind::Channel => "channel",
        }
    }

    /// Parses `app` or `channel`.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "app" => Some(EntitlementKind::App),
            "channel" => Some(EntitlementKind::Channel),
            _ => None,
        }
    }
}

/// The bearer token of a request, if any.
pub fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

/// A device that authenticated with a device token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSession {
    /// Account of the device.
    pub account_id: i64,
    /// Row of the device.
    pub device_row: i64,
    /// Key id of the device.
    pub device_id: String,
}

/// The account an **account token** stands for, and the time the token was issued. The signature
/// and expiry alone are not enough: the account must still exist, it must not be younger than the
/// token (account numbers of deleted accounts are given out again, and a token of the deleted
/// account must not speak for the new one), and the token must carry the account's current epoch
/// (a password change ends all earlier tokens).
pub fn account_token_of(st: &AppState, token: &str) -> Option<(i64, i64)> {
    let claims = auth::verify_token_claims(&st.secret, token)?;
    let issued = claims.expires - crate::accounts::TOKEN_TTL_SECS;
    let row: Option<(i64, i64)> = st
        .db
        .lock()
        .query_row(
            "SELECT created, token_epoch FROM accounts WHERE id=?1",
            [claims.account_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    row.filter(|(created, epoch)| *created <= issued && *epoch == claims.epoch)
        .map(|_| (claims.account_id, issued))
}

/// The device a **device token** stands for. Beyond signature and expiry, the device must still
/// be active in that account and both the account's and the device's epoch must be the ones the
/// token was issued under: removal, replacement and password change end the token at once.
pub fn device_of_token(st: &AppState, token: &str) -> Option<DeviceSession> {
    let claims = crate::devtoken::verify(&st.secret, token, st.devices.now_secs())?;
    let row: Option<(i64, i64, i64, i64)> = st
        .db
        .lock()
        .query_row(
            "SELECT d.id, d.state, d.epoch, a.token_epoch FROM devices d
             JOIN accounts a ON a.id = d.account_id
             WHERE d.device_id=?1 AND d.account_id=?2",
            rusqlite::params![claims.device_id, claims.account_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok();
    let (device_row, state, device_epoch, account_epoch) = row?;
    let live = state == crate::devices::DeviceState::Active.code()
        && device_epoch == claims.device_epoch
        && account_epoch == claims.account_epoch;
    live.then_some(DeviceSession {
        account_id: claims.account_id,
        device_row,
        device_id: claims.device_id,
    })
}

/// The account a bearer token stands for: an account token, or the token of one of its devices.
pub fn account_of_token(st: &AppState, token: &str) -> Option<i64> {
    account_token_of(st, token)
        .map(|(id, _)| id)
        .or_else(|| device_of_token(st, token).map(|d| d.account_id))
}

/// The account of a request with a valid token; `auth_required` otherwise.
pub fn require_account(st: &AppState, headers: &HeaderMap) -> Result<i64, ApiError> {
    bearer(headers)
        .and_then(|t| account_of_token(st, t))
        .ok_or(ApiError::AuthRequired)
}

/// Account and issue time of a request that carries a valid account token (a device token is not
/// enough); `auth_required` otherwise.
pub fn require_account_token(st: &AppState, headers: &HeaderMap) -> Result<(i64, i64), ApiError> {
    bearer(headers)
        .and_then(|t| account_token_of(st, t))
        .ok_or(ApiError::AuthRequired)
}

/// The device of a request that carries a valid device token (an account token is not enough);
/// `auth_required` otherwise.
pub fn require_device(st: &AppState, headers: &HeaderMap) -> Result<DeviceSession, ApiError> {
    bearer(headers)
        .and_then(|t| device_of_token(st, t))
        .ok_or(ApiError::AuthRequired)
}

/// True when the account holds the entitlement.
pub fn is_entitled(
    conn: &Connection,
    account_id: i64,
    kind: EntitlementKind,
    item: &str,
) -> rusqlite::Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entitlements WHERE account_id=?1 AND kind=?2 AND item=?3",
        rusqlite::params![account_id, kind.as_str(), item],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Gate for premium apps and gated channels: no valid token → `auth_required`; valid token
/// without a record → `not_entitled`. Returns the account id on success.
pub fn require_entitlement(
    st: &AppState,
    headers: &HeaderMap,
    kind: EntitlementKind,
    item: &str,
) -> Result<i64, ApiError> {
    let account_id = require_account(st, headers)?;
    if is_entitled(&st.db.lock(), account_id, kind, item)? {
        Ok(account_id)
    } else {
        Err(ApiError::NotEntitled)
    }
}

/// Records an entitlement. Returns `false` when the account already had it.
pub fn grant(
    conn: &Connection,
    account_id: i64,
    kind: EntitlementKind,
    item: &str,
) -> rusqlite::Result<bool> {
    let changed = conn.execute(
        "INSERT OR IGNORE INTO entitlements(account_id, kind, item, granted) VALUES(?1,?2,?3,?4)",
        rusqlite::params![account_id, kind.as_str(), item, db::now()],
    )?;
    Ok(changed > 0)
}

/// Removes an entitlement. Returns `false` when there was none.
pub fn revoke(
    conn: &Connection,
    account_id: i64,
    kind: EntitlementKind,
    item: &str,
) -> rusqlite::Result<bool> {
    let changed = conn.execute(
        "DELETE FROM entitlements WHERE account_id=?1 AND kind=?2 AND item=?3",
        rusqlite::params![account_id, kind.as_str(), item],
    )?;
    Ok(changed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn db_with_account() -> Db {
        let db = Db::open(":memory:").unwrap();
        db.lock()
            .execute(
                "INSERT INTO accounts(id, username, pw_hash, created) VALUES(1,'u','h',0)",
                [],
            )
            .unwrap();
        db
    }

    #[test]
    fn kind_round_trips() {
        for k in [EntitlementKind::App, EntitlementKind::Channel] {
            assert_eq!(EntitlementKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(EntitlementKind::parse("APP"), None);
    }

    #[test]
    fn grant_revoke_cycle() {
        let db = db_with_account();
        let conn = db.lock();
        assert!(!is_entitled(&conn, 1, EntitlementKind::App, "x").unwrap());
        assert!(grant(&conn, 1, EntitlementKind::App, "x").unwrap());
        assert!(!grant(&conn, 1, EntitlementKind::App, "x").unwrap());
        assert!(is_entitled(&conn, 1, EntitlementKind::App, "x").unwrap());
        assert!(
            !is_entitled(&conn, 1, EntitlementKind::Channel, "x").unwrap(),
            "kinds are separate"
        );
        assert!(!is_entitled(&conn, 2, EntitlementKind::App, "x").unwrap());
        assert!(revoke(&conn, 1, EntitlementKind::App, "x").unwrap());
        assert!(!revoke(&conn, 1, EntitlementKind::App, "x").unwrap());
        assert!(!is_entitled(&conn, 1, EntitlementKind::App, "x").unwrap());
    }

    #[test]
    fn grant_for_unknown_account_is_refused_and_delete_cascades() {
        let db = db_with_account();
        let conn = db.lock();
        assert!(
            grant(&conn, 99, EntitlementKind::App, "x").is_err(),
            "foreign key"
        );
        assert!(grant(&conn, 1, EntitlementKind::Channel, "lts").unwrap());
        conn.execute("DELETE FROM accounts WHERE id=1", []).unwrap();
        assert!(!is_entitled(&conn, 1, EntitlementKind::Channel, "lts").unwrap());
    }

    #[test]
    fn bearer_parsing() {
        let mut h = HeaderMap::new();
        assert_eq!(bearer(&h), None);
        h.insert("authorization", "Basic abc".parse().unwrap());
        assert_eq!(bearer(&h), None);
        h.insert("authorization", "Bearer tok".parse().unwrap());
        assert_eq!(bearer(&h), Some("tok"));
    }
}
