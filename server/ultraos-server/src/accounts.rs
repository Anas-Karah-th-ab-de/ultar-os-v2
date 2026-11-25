//! Account-bound endpoints: registration, login, password change, TURN credentials, WireGuard
//! peer. Devices and their numbers live in [`crate::device_api`].
//!
//! These keep the behaviour of the first server release; an account stays required for calls via
//! Ultar number, VPN, backup, mesh over the internet and premium content.

use crate::entitlements::require_account;
use crate::error::ApiError;
use crate::guard::ClientAddr;
use crate::{auth, db, AppState};
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::{async_trait, Extension, Json};
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

/// Most WireGuard peers one account may hold (each takes an address of the shared 10.80.0.0/16 pool for good).
pub const MAX_VPN_PEERS_PER_ACCOUNT: i64 = 16;
/// Password hashes computed at the same time. Argon2id needs about 19 MiB per computation; without a
/// bound, concurrent register/login requests from many addresses decide how much memory the server uses.
const HASH_SLOTS: usize = 4;
static HASHING: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(HASH_SLOTS);

/// A stored hash that matches no password: verified for unknown users so that the answer takes as
/// long as for a known user with a wrong password.
fn decoy_hash() -> &'static str {
    static DECOY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DECOY.get_or_init(|| auth::hash_password("decoy-password-never-accepted").unwrap_or_default())
}

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

/// Body of register and login.
#[derive(Deserialize)]
pub struct Credentials {
    username: String,
    password: String,
}

fn session(st: &AppState, account_id: i64, epoch: i64) -> Result<Json<Value>, ApiError> {
    let token = auth::issue_token_epoch(&st.secret, account_id, TOKEN_TTL_SECS, epoch)
        .ok_or(ApiError::Internal)?;
    Ok(Json(json!({ "account_id": account_id, "token": token })))
}

async fn hash_new_password(password: String) -> Result<String, ApiError> {
    let _slot = HASHING.acquire().await.map_err(|_| ApiError::Internal)?;
    tokio::task::spawn_blocking(move || auth::hash_password(&password))
        .await
        .map_err(|_| ApiError::Internal)?
        .map_err(|_| ApiError::Internal)
}

/// Who asks for a password check: the client address, and whether the request carries the
/// token of a managing device of the account (exempt from the account-wide budget).
#[derive(Debug, Clone, Copy)]
pub struct Asker {
    /// Address of the client (as the guard determined it).
    pub client: std::net::IpAddr,
    /// True when the request is authenticated by a managing device of the account.
    pub managing: bool,
}

/// Verifies `password` against `hash` (Argon2id, bounded concurrency). When the account is known
/// the check is booked on its budgets first ([`crate::pwguard`]): an exhausted budget answers
/// `rate_limited` without looking at the password; a strained account budget makes the check
/// wait.
async fn verify_guarded(
    st: &AppState,
    account_id: Option<i64>,
    asker: Asker,
    hash: String,
    password: String,
) -> Result<bool, ApiError> {
    let booking = match account_id {
        Some(id) => {
            let now = st.devices.now_ms();
            let booked = st
                .devices
                .passwords
                .book(id, asker.client, asker.managing, now);
            Some(booked.map_err(|retry_after_secs| ApiError::RateLimited { retry_after_secs })?)
        }
        None => None,
    };
    if let Some(delay) = booking.map(|b| b.delay).filter(|d| !d.is_zero()) {
        tokio::time::sleep(delay).await;
    }
    let _slot = HASHING.acquire().await.map_err(|_| ApiError::Internal)?;
    let ok = tokio::task::spawn_blocking(move || auth::verify_password(&password, &hash))
        .await
        .map_err(|_| ApiError::Internal)?;
    let ok = ok && account_id.is_some();
    if let (true, Some(booking)) = (ok, booking) {
        st.devices.passwords.succeeded(&booking);
    }
    Ok(ok)
}

/// Checks `password` against the stored hash of `account_id`. An unknown account costs the same
/// work as a wrong password. Failed checks count against the account's budgets.
pub async fn password_matches(
    st: &AppState,
    account_id: i64,
    asker: Asker,
    password: String,
) -> Result<bool, ApiError> {
    if password.len() > 256 {
        return Ok(false);
    }
    let stored: Option<String> = st
        .db
        .lock()
        .query_row(
            "SELECT pw_hash FROM accounts WHERE id=?1",
            [account_id],
            |r| r.get(0),
        )
        .optional()?;
    let known = stored.is_some().then_some(account_id);
    let hash = stored.unwrap_or_else(|| decoy_hash().to_string());
    verify_guarded(st, known, asker, hash, password).await
}

/// The asker of a request: client address from the guard, managing device from the token.
pub fn asker_of(st: &AppState, headers: &HeaderMap, client: Option<std::net::IpAddr>) -> Asker {
    Asker {
        client: client.unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)),
        managing: crate::device_api::is_managing_token(st, headers),
    }
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
    let hash = hash_new_password(c.password).await?;
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
    session(&st, id, 0)
}

/// `POST /v1/account/login` — checks the password and returns a session token.
pub async fn account_login(
    State(st): State<Shared>,
    client: Option<Extension<ClientAddr>>,
    ApiJson(c): ApiJson<Credentials>,
) -> Result<Json<Value>, ApiError> {
    if c.username.len() > 256 || c.password.len() > 256 {
        return Err(ApiError::InvalidCredentials);
    }
    let row: Option<(i64, String, i64)> = st
        .db
        .lock()
        .query_row(
            "SELECT id, pw_hash, token_epoch FROM accounts WHERE username=?1",
            [&c.username],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    // unknown user: the same work as for a wrong password (no timing difference to tell them apart)
    let (id, hash) = match row {
        Some((id, hash, epoch)) => (Some((id, epoch)), hash),
        None => (None, decoy_hash().to_string()),
    };
    let asker = Asker {
        client: client.map_or(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), |c| {
            c.0 .0
        }),
        managing: false,
    };
    let ok = verify_guarded(&st, id.map(|(id, _)| id), asker, hash, c.password).await?;
    match id {
        Some((id, epoch)) if ok => session(&st, id, epoch),
        _ => Err(ApiError::InvalidCredentials),
    }
}

/// Body of the password change.
#[derive(Deserialize)]
pub struct PasswordChange {
    old_password: String,
    new_password: String,
}

/// `POST /v1/account/password` — changes the password. The account's token epoch moves on, so
/// every account token and every device token issued before is dead at once; a pending
/// lost-device recovery is dropped; the devices are told. Returns a new account token.
pub async fn account_password(
    State(st): State<Shared>,
    h: HeaderMap,
    client: Option<Extension<ClientAddr>>,
    ApiJson(p): ApiJson<PasswordChange>,
) -> Result<Json<Value>, ApiError> {
    let acc = require_account(&st, &h)?;
    if !(8..=256).contains(&p.new_password.len()) {
        return Err(ApiError::InvalidRequest);
    }
    let asker = asker_of(&st, &h, client.map(|c| c.0 .0));
    if !password_matches(&st, acc, asker, p.old_password).await? {
        return Err(ApiError::InvalidCredentials);
    }
    let hash = hash_new_password(p.new_password).await?;
    let now = st.devices.now_secs();
    let epoch: i64 = {
        let mut conn = st.db.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE accounts SET pw_hash=?1, token_epoch=token_epoch+1 WHERE id=?2",
            rusqlite::params![hash, acc],
        )?;
        crate::device_moves::recovery_clear(&tx, acc)?;
        crate::device_events::push(
            &tx,
            acc,
            crate::device_events::EventKind::PasswordChanged,
            "",
            now,
        )?;
        let epoch = tx.query_row("SELECT token_epoch FROM accounts WHERE id=?1", [acc], |r| {
            r.get(0)
        })?;
        tx.commit()?;
        epoch
    };
    st.hub
        .notify_account(st.devices.now_secs(), acc, "password_changed", "");
    tracing::info!(event = "password_changed");
    session(&st, acc, epoch)
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

/// `GET /v1/turn-cred` — short-lived TURN credentials (coturn REST scheme: user = `exp:tag`,
/// password = base64(HMAC-SHA1(shared secret, user))). The tag is random for every credential:
/// the TURN server learns nothing about the account, and two calls of one account cannot be
/// linked through it.
pub async fn turn_cred(State(st): State<Shared>, h: HeaderMap) -> Result<Json<Value>, ApiError> {
    use hmac::{Hmac, Mac};
    require_account(&st, &h)?;
    if st.turn_secret.is_empty() {
        return Err(ApiError::TurnNotConfigured);
    }
    let exp = db::now() + 3600;
    let mut tag = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut tag);
    let user = format!("{exp}:{}", hex::encode(tag));
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
            let held: i64 = conn.query_row(
                "SELECT COUNT(*) FROM wg_peers WHERE account_id=?1",
                [acc],
                |r| r.get(0),
            )?;
            if held >= MAX_VPN_PEERS_PER_ACCOUNT {
                return Err(ApiError::VpnPoolExhausted);
            }
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
