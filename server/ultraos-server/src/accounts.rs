//! Account-bound endpoints: registration, login, device number, TURN credentials, WireGuard peer.
//!
//! These keep the behaviour of the first server release; an account stays required for calls via
//! Ultar number, VPN, backup, mesh over the internet and premium content.

use crate::entitlements::require_account;
use crate::error::ApiError;
use crate::{auth, db, AppState};
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::{async_trait, Json};
use rand::Rng;
use rusqlite::OptionalExtension;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;

type Shared = Arc<AppState>;

/// Lifetime of a session token in seconds (30 days).
pub const TOKEN_TTL_SECS: i64 = 30 * 86400;
/// Highest host number of the internal WireGuard pool 10.80.0.0/16.
const WG_POOL_LAST: u32 = 0xFFFE;

/// JSON body extractor whose failures are typed API errors.
pub struct ApiJson<T>(pub T);

#[async_trait]
impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(ApiJson(value)),
            Err(rejection) => Err(json_rejection(&rejection)),
        }
    }
}

fn json_rejection(rejection: &JsonRejection) -> ApiError {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::PayloadTooLarge
    } else {
        ApiError::InvalidRequest
    }
}

fn gen_number() -> String {
    let mut r = rand::thread_rng();
    format!("UOS-{:04X}-{:04X}", r.gen::<u16>(), r.gen::<u16>())
}

/// Body of register and login.
#[derive(Deserialize)]
pub struct Credentials {
    username: String,
    password: String,
}

fn session(st: &AppState, account_id: i64) -> Result<Json<Value>, ApiError> {
    let token =
        auth::issue_token(&st.secret, account_id, TOKEN_TTL_SECS).ok_or(ApiError::Internal)?;
    Ok(Json(json!({ "account_id": account_id, "token": token })))
}

/// `POST /v1/account/register` — creates an account and returns a session token.
pub async fn account_register(
    State(st): State<Shared>,
    ApiJson(c): ApiJson<Credentials>,
) -> Result<Json<Value>, ApiError> {
    let name_len = c.username.chars().count();
    if !(3..=64).contains(&name_len) || !(8..=256).contains(&c.password.len()) {
        return Err(ApiError::InvalidRequest);
    }
    let password = c.password;
    let hash = tokio::task::spawn_blocking(move || auth::hash_password(&password))
        .await
        .map_err(|_| ApiError::Internal)?
        .map_err(|_| ApiError::Internal)?;
    let id = {
        let conn = st.db.lock();
        let inserted = conn.execute(
            "INSERT INTO accounts(username, pw_hash, created) VALUES(?1,?2,?3)",
            rusqlite::params![c.username, hash, db::now()],
        );
        if inserted.is_err() {
            return Err(ApiError::UsernameTaken);
        }
        conn.last_insert_rowid()
    };
    tracing::info!(event = "account_registered");
    session(&st, id)
}

/// `POST /v1/account/login` — checks the password and returns a session token.
pub async fn account_login(
    State(st): State<Shared>,
    ApiJson(c): ApiJson<Credentials>,
) -> Result<Json<Value>, ApiError> {
    if c.username.len() > 256 || c.password.len() > 256 {
        return Err(ApiError::InvalidCredentials);
    }
    let row: Option<(i64, String)> = st
        .db
        .lock()
        .query_row(
            "SELECT id, pw_hash FROM accounts WHERE username=?1",
            [&c.username],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((id, hash)) = row else {
        return Err(ApiError::InvalidCredentials);
    };
    let password = c.password;
    let ok = tokio::task::spawn_blocking(move || auth::verify_password(&password, &hash))
        .await
        .map_err(|_| ApiError::Internal)?;
    if !ok {
        return Err(ApiError::InvalidCredentials);
    }
    session(&st, id)
}

/// `GET /v1/account/entitlements` — what the signed-in account has access to:
/// `{"apps":[ids],"channels":[names]}`. Lets the store show "unlocked" without trial downloads.
pub async fn account_entitlements(
    State(st): State<Shared>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let acc = require_account(&st, &h)?;
    let conn = st.db.lock();
    let mut stmt = conn.prepare_cached(
        "SELECT kind, item FROM entitlements WHERE account_id = ?1 ORDER BY kind, item LIMIT 1000",
    )?;
    let rows = stmt.query_map([acc], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let (mut apps, mut channels) = (Vec::new(), Vec::new());
    for row in rows {
        let (kind, item) = row?;
        if kind == "app" {
            apps.push(item);
        } else {
            channels.push(item);
        }
    }
    Ok(Json(json!({ "apps": apps, "channels": channels })))
}

/// Body of the device registration.
#[derive(Deserialize)]
pub struct DeviceReg {
    device_id: String,
    pubkey: String,
}

/// `POST /v1/device/register` — registers the device and assigns its unique Ultar number.
/// Idempotent per device. A device id owned by another account is refused (`device_taken`).
pub async fn device_register(
    State(st): State<Shared>,
    h: HeaderMap,
    ApiJson(d): ApiJson<DeviceReg>,
) -> Result<Json<Value>, ApiError> {
    let acc = require_account(&st, &h)?;
    if d.device_id.is_empty() || d.device_id.len() > 128 || d.pubkey.len() > 512 {
        return Err(ApiError::InvalidRequest);
    }
    let conn = st.db.lock();
    conn.execute(
        "INSERT OR IGNORE INTO devices(account_id, device_id, pubkey, created) VALUES(?1,?2,?3,?4)",
        rusqlite::params![acc, d.device_id, d.pubkey, db::now()],
    )?;
    let owner: i64 = conn.query_row(
        "SELECT account_id FROM devices WHERE device_id=?1",
        [&d.device_id],
        |r| r.get(0),
    )?;
    if owner != acc {
        return Err(ApiError::DeviceTaken);
    }
    let existing: Option<String> = conn
        .query_row(
            "SELECT number FROM numbers WHERE device_id=?1",
            [&d.device_id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(n) = existing {
        return Ok(Json(json!({ "number": n })));
    }
    for _ in 0..20 {
        let n = gen_number();
        let inserted = conn.execute(
            "INSERT INTO numbers(number, account_id, device_id, created) VALUES(?1,?2,?3,?4)",
            rusqlite::params![n, acc, d.device_id, db::now()],
        );
        if inserted.is_ok() {
            return Ok(Json(json!({ "number": n })));
        }
    }
    Err(ApiError::Internal)
}

/// `GET /v1/turn-cred` — short-lived TURN credentials (coturn REST scheme: user = `exp:account`,
/// password = base64(HMAC-SHA1(shared secret, user))).
pub async fn turn_cred(State(st): State<Shared>, h: HeaderMap) -> Result<Json<Value>, ApiError> {
    use hmac::{Hmac, Mac};
    let acc = require_account(&st, &h)?;
    if st.turn_secret.is_empty() {
        return Err(ApiError::TurnNotConfigured);
    }
    let exp = db::now() + 3600;
    let user = format!("{exp}:{acc}");
    let mut mac = <Hmac<sha1::Sha1> as Mac>::new_from_slice(st.turn_secret.as_bytes())
        .map_err(|_| ApiError::Internal)?;
    mac.update(user.as_bytes());
    let pass = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        mac.finalize().into_bytes(),
    );
    let host = &st.turn_host;
    Ok(Json(json!({
        "urls": [
            format!("stun:{host}:3478"),
            format!("turn:{host}:3478?transport=udp"),
            format!("turn:{host}:3478?transport=tcp")
        ],
        "username": user,
        "credential": pass,
        "ttl": 3600
    })))
}

/// Body of the WireGuard peer request.
#[derive(Deserialize)]
pub struct PeerReq {
    pubkey: String,
}

fn valid_wireguard_key(key: &str) -> bool {
    key.len() == 44
        && key.ends_with('=')
        && base64::Engine::decode(&base64::engine::general_purpose::STANDARD, key)
            .map(|v| v.len() == 32)
            .unwrap_or(false)
}

/// `POST /v1/vpn/peer` — provisions a WireGuard peer for the account (same key → same address).
/// The host service `ultar-wg-sync` copies new peers from the database to `wg0`.
pub async fn vpn_peer(
    State(st): State<Shared>,
    h: HeaderMap,
    ApiJson(p): ApiJson<PeerReq>,
) -> Result<Json<Value>, ApiError> {
    let acc = require_account(&st, &h)?;
    if !valid_wireguard_key(&p.pubkey) {
        return Err(ApiError::InvalidWireguardKey);
    }
    let conn = st.db.lock();
    let existing: Option<String> = conn
        .query_row(
            "SELECT address FROM wg_peers WHERE account_id=?1 AND pubkey=?2",
            rusqlite::params![acc, p.pubkey],
            |r| r.get(0),
        )
        .optional()?;
    let address = match existing {
        Some(a) => a,
        None => {
            let n = st.wg_next.fetch_add(1, Ordering::SeqCst);
            if n > WG_POOL_LAST {
                st.wg_next.store(WG_POOL_LAST + 1, Ordering::SeqCst);
                return Err(ApiError::VpnPoolExhausted);
            }
            let a = format!("10.80.{}.{}/32", (n >> 8) & 0xff, n & 0xff);
            conn.execute(
                "INSERT OR REPLACE INTO wg_peers(account_id,pubkey,address,created) VALUES(?1,?2,?3,?4)",
                rusqlite::params![acc, p.pubkey, a, db::now()],
            )?;
            a
        }
    };
    Ok(Json(json!({
        "server_pubkey": st.wg_server_pubkey,
        "endpoint": st.wg_endpoint,
        "address": address,
        "dns": "10.80.0.1",
        "allowed_ips": "0.0.0.0/0, ::/0"
    })))
}

/// The next free host number of the WireGuard pool, derived from the stored peers.
pub fn next_wg_host(conn: &rusqlite::Connection) -> rusqlite::Result<u32> {
    let mut stmt = conn.prepare("SELECT address FROM wg_peers")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut max = 1u32;
    for address in rows {
        let address = address?;
        let ip = address.split('/').next().unwrap_or_default();
        let octets: Vec<u32> = ip.split('.').filter_map(|x| x.parse().ok()).collect();
        if octets.len() == 4 {
            max = max.max((octets[2] << 8) | octets[3]);
        }
    }
    Ok(max + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_have_the_documented_shape() {
        let n = gen_number();
        assert_eq!(n.len(), 13);
        assert!(n.starts_with("UOS-") && n.as_bytes()[8] == b'-');
        assert!(n[4..8]
            .bytes()
            .chain(n[9..].bytes())
            .all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn wireguard_key_validation() {
        assert!(valid_wireguard_key(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        ));
        assert!(!valid_wireguard_key(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        ));
        assert!(!valid_wireguard_key("short="));
        assert!(!valid_wireguard_key(
            "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!="
        ));
    }

    #[test]
    fn next_wg_host_continues_after_the_highest_address() {
        let db = crate::db::Db::open(":memory:").unwrap();
        let conn = db.lock();
        assert_eq!(next_wg_host(&conn).unwrap(), 2);
        conn.execute_batch(
            "INSERT INTO accounts(id, username, pw_hash, created) VALUES(1,'u','h',0);
             INSERT INTO wg_peers VALUES(1,'k1','10.80.0.7/32',0);
             INSERT INTO wg_peers VALUES(1,'k2','10.80.1.2/32',0);
             INSERT INTO wg_peers VALUES(1,'k3','garbage',0);",
        )
        .unwrap();
        assert_eq!(next_wg_host(&conn).unwrap(), 259);
    }
}
