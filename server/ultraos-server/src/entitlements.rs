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

/// The account a session token stands for. The signature and expiry alone are not enough: the
/// account must still exist, and it must not be younger than the token (account numbers of deleted
/// accounts are given out again, and a token of the deleted account must not speak for the new one).
pub fn account_of_token(st: &AppState, token: &str) -> Option<i64> {
    let (id, expires) = auth::verify_token_expiry(&st.secret, token)?;
    let issued = expires - crate::accounts::TOKEN_TTL_SECS;
    let created: Option<i64> = st
        .db
        .lock()
        .query_row("SELECT created FROM accounts WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .ok();
    created.filter(|c| *c <= issued).map(|_| id)
}

/// The account of a request with a valid token; `auth_required` otherwise.
pub fn require_account(st: &AppState, headers: &HeaderMap) -> Result<i64, ApiError> {
    bearer(headers)
        .and_then(|t| account_of_token(st, t))
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
