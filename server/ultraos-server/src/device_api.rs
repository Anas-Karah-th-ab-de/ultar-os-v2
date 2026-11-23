//! HTTP endpoints of device identity (`docs/DEVICE-IDENTITY.md` §5): challenge, registration with
//! proof of possession, device tokens, the device list, rename, remove, replace, recovery of a
//! lost device and the upgrade of registrations from before device keys.
//!
//! Order in every handler: authenticate the caller, parse every field, consume the challenge,
//! verify the signature(s), then change the database in one transaction. Nothing is written
//! before all checks passed. Logs name devices by a short prefix of their id only.

use crate::accounts::ApiJson;
use crate::challenge::{ChallengeUse, MAX_ID_BYTES, SESSION_ID_BYTES};
use crate::device_moves::{self, LegacyClaim};
use crate::devices::{self, DeviceState, Registered};
use crate::devtoken::{self, DeviceClaims, DEVICE_TOKEN_TTL_SECS};
use crate::entitlements::{
    bearer, device_of_token, require_account, require_account_token, require_device,
};
use crate::error::ApiError;
use crate::identity::{
    self, Purpose, Transcript, LOG_PREFIX_LEN, PUBLIC_KEY_BYTES, SIGNATURE_BYTES,
};
use crate::{device_events, AppState};
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use subtle::ConstantTimeEq;

pub(crate) type Shared = Arc<AppState>;
pub(crate) type Reply = Result<Json<Value>, ApiError>;

pub(crate) fn short(device_id: &str) -> String {
    identity::id_prefix(device_id, LOG_PREFIX_LEN)
}

pub(crate) fn notice_prefix(device_id: &str) -> String {
    identity::id_prefix(device_id, device_events::NOTICE_PREFIX_LEN)
}

/// A proof of possession as it arrives: which challenge, and the signature over its transcript.
pub(crate) struct Proof {
    pub(crate) challenge_id: Vec<u8>,
    pub(crate) signature: [u8; SIGNATURE_BYTES],
}

impl Proof {
    /// Malformed text is `invalid_request`; well-formed ids of a length the server never issues
    /// are left to the store (`challenge_unknown`).
    pub(crate) fn parse(challenge_id: &str, signature: &str) -> Result<Self, ApiError> {
        use base64::Engine;
        if challenge_id.len() > MAX_ID_BYTES * 2 {
            return Err(ApiError::InvalidRequest);
        }
        let id = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(challenge_id)
            .map_err(|_| ApiError::InvalidRequest)?;
        Ok(Self {
            challenge_id: id,
            signature: identity::parse_signature(signature)?,
        })
    }
}

/// Consumes the challenge of `proof` and verifies its signature under the key the challenge was
/// issued for. Returns that key and the account the challenge is bound to.
pub(crate) fn proven(
    st: &AppState,
    proof: &Proof,
    usage: ChallengeUse,
    account_id: Option<i64>,
    purpose: Purpose,
    extra: &[u8],
) -> Result<crate::challenge::Bound, ApiError> {
    let store = &st.devices.challenges;
    let now = st.devices.now_ms();
    let verify = |bound: &crate::challenge::Bound| {
        let transcript = Transcript {
            purpose,
            server: &st.devices.config.server_name,
            account_id: bound.account_id,
            challenge: &bound.challenge,
            subject_key: &bound.public_key,
            extra,
        };
        transcript.verify(&proof.signature)
    };
    if proof.challenge_id.len() == SESSION_ID_BYTES {
        // Session challenges are consumed only by a signature that verifies: a stranger who
        // knows the key can neither use them up nor fill the device's budget.
        let ticket = store.open_session(&proof.challenge_id, usage, account_id, now)?;
        verify(&ticket.bound)?;
        store.confirm(&ticket, now)?;
        return Ok(ticket.bound);
    }
    let bound = store.take(&proof.challenge_id, usage, account_id, now)?;
    verify(&bound)?;
    Ok(bound)
}

/// The answer to a registration, upgrade or move: ids, number and a device token.
pub(crate) fn registered_reply(st: &AppState, account_id: i64, done: &Registered) -> Reply {
    let account_epoch =
        devices::account_epoch(&st.db.lock(), account_id)?.ok_or(ApiError::AuthRequired)?;
    let claims = DeviceClaims {
        account_id,
        device_id: done.device_id.clone(),
        expires: st.devices.now_secs() + DEVICE_TOKEN_TTL_SECS,
        account_epoch,
        device_epoch: done.epoch,
    };
    let token = devtoken::issue(&st.secret, &claims).ok_or(ApiError::Internal)?;
    Ok(Json(json!({
        "device_id": done.device_id,
        "number": done.number,
        "account_id": account_id,
        "token": token,
        "expires_in": DEVICE_TOKEN_TTL_SECS,
        "manage": done.manage_in == 0,
        "manage_in": done.manage_in,
        "recovery": recovery_json(st, account_id)?,
        "unapproved": unapproved_json(st, account_id, &done.device_id)?,
    })))
}

/// The other devices of the account that were added and may not manage devices yet:
/// `[{"device":"<id>","manage_in":<seconds>}]`. Like a pending recovery this is a state the
/// owner's devices show until it is over (approved, removed, or the cool-down passed).
pub(crate) fn unapproved_json(
    st: &AppState,
    account_id: i64,
    except: &str,
) -> Result<Value, ApiError> {
    let (now, cooldown) = (
        st.devices.now_secs(),
        st.devices.config.recovery_cooldown_secs,
    );
    let list = devices::list(&st.db.lock(), account_id, now, cooldown)?;
    let waiting: Vec<Value> = list
        .into_iter()
        .filter(|d| d.device_id != except)
        .filter_map(|d| {
            let wait = d.manage_in.filter(|w| *w > 0)?;
            Some(json!({"device": d.device_id, "manage_in": wait}))
        })
        .collect();
    Ok(Value::Array(waiting))
}

/// The pending recovery of the account as it is reported to its devices (`null` when none): a
/// recovery is a state, not only a notice, so every answer a device gets about itself carries it.
pub(crate) fn recovery_json(st: &AppState, account_id: i64) -> Result<Value, ApiError> {
    let info = device_moves::recovery_info(&st.db.lock(), account_id, st.devices.now_secs())?;
    Ok(match info {
        Some(r) => json!({"old_device": r.old_device, "eligible_in": r.eligible_in}),
        None => Value::Null,
    })
}

/// True when the request's bearer token is the device token of a device that may manage the
/// account's devices now.
pub(crate) fn is_managing_token(st: &AppState, headers: &HeaderMap) -> bool {
    let Some(me) = bearer(headers).and_then(|t| device_of_token(st, t)) else {
        return false;
    };
    managing_device(&st.db.lock(), st, &me).is_ok()
}

/// The calling device's row, if it may manage other devices of its account now
/// (`device_not_approved` otherwise).
pub(crate) fn managing_device(
    conn: &rusqlite::Connection,
    st: &AppState,
    me: &crate::entitlements::DeviceSession,
) -> Result<devices::DeviceRow, ApiError> {
    let row = devices::by_row(conn, me.device_row)?
        .filter(|d| d.account_id == me.account_id)
        .ok_or(ApiError::AuthRequired)?;
    row.require_manager(
        st.devices.now_secs(),
        st.devices.config.recovery_cooldown_secs,
    )?;
    Ok(row)
}

/// Body of the challenge request.
#[derive(Deserialize)]
pub struct ChallengeReq {
    purpose: String,
    public_key: String,
}

/// The account a session challenge for `key` is bound to: the key must be an active device.
fn session_account(st: &AppState, key: &[u8; PUBLIC_KEY_BYTES]) -> Result<i64, ApiError> {
    let device =
        devices::by_id(&st.db.lock(), &identity::device_id(key))?.ok_or(ApiError::DeviceUnknown)?;
    match device.state {
        DeviceState::Active => Ok(device.account_id),
        DeviceState::Legacy => Err(ApiError::DeviceUnknown),
        DeviceState::Removed | DeviceState::Replaced => Err(ApiError::DeviceRetired),
    }
}

/// `POST /v1/device/challenge` — issues a single-use challenge bound to purpose, account and the
/// claimed public key. Purpose `session` needs no token (the account is the key's own).
pub async fn challenge(
    State(st): State<Shared>,
    h: HeaderMap,
    ApiJson(c): ApiJson<ChallengeReq>,
) -> Reply {
    let usage = ChallengeUse::parse(&c.purpose).ok_or(ApiError::InvalidRequest)?;
    let key = identity::parse_public_key(&c.public_key)?;
    let account_id = match usage {
        ChallengeUse::Session => session_account(&st, &key)?,
        _ => require_account(&st, &h)?,
    };
    let issued = st
        .devices
        .challenges
        .issue(usage, account_id, key, st.devices.now_ms())?;
    Ok(Json(json!({
        "challenge_id": identity::encode(&issued.id),
        "challenge": identity::encode(&issued.challenge),
        "server": st.devices.config.server_name,
        "account_id": account_id,
        "expires_in": st.devices.challenges.ttl_secs(),
    })))
}

/// Body of the registration. `challenge_id` + `signature` is the registration with proof of
/// possession; `device_id` (+ `pubkey`) is the form of the first server release.
#[derive(Deserialize)]
pub struct RegisterReq {
    challenge_id: Option<String>,
    signature: Option<String>,
    #[serde(default)]
    name: String,
    device_id: Option<String>,
}

/// `POST /v1/device/register` — registers the device key proven by the signed challenge and
/// assigns its Ultar number. Idempotent per key. The first-release form only reads the number of
/// an existing legacy registration of the caller; it can no longer create anything.
pub async fn register(
    State(st): State<Shared>,
    h: HeaderMap,
    ApiJson(r): ApiJson<RegisterReq>,
) -> Reply {
    let acc = require_account(&st, &h)?;
    let (Some(challenge_id), Some(signature)) = (&r.challenge_id, &r.signature) else {
        let legacy_id = r.device_id.as_deref().ok_or(ApiError::InvalidRequest)?;
        if legacy_id.is_empty() || legacy_id.len() > 128 {
            return Err(ApiError::InvalidRequest);
        }
        return match devices::legacy_number(&st.db.lock(), acc, legacy_id)? {
            Some(number) => Ok(Json(json!({ "number": number }))),
            None => Err(ApiError::DeviceKeyRequired),
        };
    };
    let proof = Proof::parse(challenge_id, signature)?;
    devices::clean_name(&r.name)?;
    let bound = proven(
        &st,
        &proof,
        ChallengeUse::Register,
        Some(acc),
        Purpose::Register,
        b"",
    )?;
    let now = st.devices.now_secs();
    let done = devices::register(
        &mut st.db.lock(),
        acc,
        &bound.public_key,
        &r.name,
        now,
        st.devices.limits(),
    )?;
    if done.created {
        st.hub.notify_account(
            st.devices.now_secs(),
            acc,
            "device_added",
            &notice_prefix(&done.device_id),
        );
        tracing::info!(event = "device_registered", device = %short(&done.device_id));
    }
    registered_reply(&st, acc, &done)
}

/// Body of the session request.
#[derive(Deserialize)]
pub struct SessionReq {
    challenge_id: String,
    signature: String,
}

/// `POST /v1/device/session` — issues a device token against a fresh signed challenge.
pub async fn session(State(st): State<Shared>, ApiJson(r): ApiJson<SessionReq>) -> Reply {
    let proof = Proof::parse(&r.challenge_id, &r.signature)?;
    let bound = proven(
        &st,
        &proof,
        ChallengeUse::Session,
        None,
        Purpose::Session,
        b"",
    )?;
    let device_id = identity::device_id(&bound.public_key);
    let done = {
        let conn = st.db.lock();
        // State may have changed since the challenge was issued: read it again.
        let device = devices::by_id(&conn, &device_id)?
            .filter(|d| d.account_id == bound.account_id && d.state == DeviceState::Active)
            .filter(|d| {
                d.public_key
                    .is_some_and(|k| bool::from(k.ct_eq(&bound.public_key)))
            })
            .ok_or(ApiError::DeviceUnknown)?;
        let number = devices::number_of(&conn, device.row)?.ok_or(ApiError::DeviceUnknown)?;
        devices::touch(&conn, device.row, st.devices.now_secs())?;
        Registered {
            device_row: device.row,
            device_id,
            number,
            epoch: device.epoch,
            created: false,
            manage_in: device
                .manage_in(
                    st.devices.now_secs(),
                    st.devices.config.recovery_cooldown_secs,
                )
                .unwrap_or(0),
        }
    };
    registered_reply(&st, bound.account_id, &done)
}

/// Body of the legacy upgrade.
#[derive(Deserialize)]
pub struct UpgradeReq {
    legacy_device_id: String,
    challenge_id: String,
    signature: String,
    #[serde(default)]
    name: String,
}

/// `POST /v1/device/upgrade` — gives a registration from before device keys its key, in place.
/// Needs the account token (not a device token) of the owning account, issued before the
/// migration, and a proof of possession that names the legacy id.
pub async fn upgrade(
    State(st): State<Shared>,
    h: HeaderMap,
    ApiJson(r): ApiJson<UpgradeReq>,
) -> Reply {
    let (acc, token_issued) = require_account_token(&st, &h)?;
    let proof = Proof::parse(&r.challenge_id, &r.signature)?;
    if r.legacy_device_id.is_empty() || r.legacy_device_id.len() > 128 {
        return Err(ApiError::InvalidRequest);
    }
    devices::clean_name(&r.name)?;
    let extra = r.legacy_device_id.as_bytes();
    let bound = proven(
        &st,
        &proof,
        ChallengeUse::Upgrade,
        Some(acc),
        Purpose::Upgrade,
        extra,
    )?;
    let claim = LegacyClaim {
        account_id: acc,
        legacy_id: &r.legacy_device_id,
        token_issued,
    };
    let done = device_moves::upgrade_legacy(
        &mut st.db.lock(),
        claim,
        &bound.public_key,
        &r.name,
        st.devices.now_secs(),
        st.devices.limits(),
    )?;
    st.hub.notify_account(
        st.devices.now_secs(),
        acc,
        "device_upgraded",
        &notice_prefix(&done.device_id),
    );
    tracing::info!(event = "device_upgraded", device = %short(&done.device_id));
    registered_reply(&st, acc, &done)
}

/// `GET /v1/devices` — the devices of the account: name, number, state, day added, day last seen,
/// whether each may manage the others, and the pending recovery if there is one.
pub async fn list(State(st): State<Shared>, h: HeaderMap) -> Reply {
    let acc = require_account(&st, &h)?;
    let current = bearer(&h)
        .and_then(|t| device_of_token(&st, t))
        .map(|d| d.device_id);
    let (now, cooldown) = (
        st.devices.now_secs(),
        st.devices.config.recovery_cooldown_secs,
    );
    let entries: Vec<Value> = devices::list(&st.db.lock(), acc, now, cooldown)?
        .into_iter()
        .map(|d| {
            json!({
                "device_id": d.device_id,
                "name": d.name,
                "number": d.number,
                "state": d.state.as_str(),
                "added": d.added,
                "last_seen": d.last_seen,
                "current": current.as_deref() == Some(d.device_id.as_str()),
                "manage": d.manage_in == Some(0),
                "manage_in": d.manage_in,
                "public_key": d.public_key.map(|k| identity::encode(&k)),
            })
        })
        .collect();
    Ok(Json(json!({
        "devices": entries,
        "limit": st.devices.config.max_devices,
        "recovery": recovery_json(&st, acc)?,
        "unapproved": unapproved_json(&st, acc, "")?,
    })))
}

/// Body of the rename.
#[derive(Deserialize)]
pub struct RenameReq {
    device_id: String,
    name: String,
}

/// `POST /v1/device/rename` — renames a device of the account. Needs the device token of a
/// managing device.
pub async fn rename(
    State(st): State<Shared>,
    h: HeaderMap,
    ApiJson(r): ApiJson<RenameReq>,
) -> Reply {
    let me = require_device(&st, &h)?;
    let conn = st.db.lock();
    managing_device(&conn, &st, &me)?;
    devices::rename(&conn, me.account_id, &r.device_id, &r.name)?;
    Ok(Json(json!({ "ok": true })))
}

/// Body that names one device.
#[derive(Deserialize)]
pub struct DeviceRef {
    device_id: String,
}

/// `POST /v1/device/remove` — removes a device of the account. Its key and number are retired,
/// its tokens stop working, its signalling socket is closed. Every device may remove itself;
/// removing another device needs the device token of a managing device.
pub async fn remove(
    State(st): State<Shared>,
    h: HeaderMap,
    ApiJson(r): ApiJson<DeviceRef>,
) -> Reply {
    let me = require_device(&st, &h)?;
    let now = st.devices.now_secs();
    let number = {
        let mut conn = st.db.lock();
        if r.device_id != me.device_id {
            managing_device(&conn, &st, &me)?;
        }
        devices::remove(&mut conn, me.account_id, &r.device_id, now)?
    };
    if let Some(number) = &number {
        st.hub.kick(number);
    }
    st.hub.notify_account(
        st.devices.now_secs(),
        me.account_id,
        "device_removed",
        &notice_prefix(&r.device_id),
    );
    tracing::info!(event = "device_removed", device = %short(&r.device_id));
    Ok(Json(json!({ "removed": true })))
}

/// Query of the notices endpoint.
#[derive(Deserialize)]
pub struct EventsQuery {
    after: Option<i64>,
}

/// `GET /v1/device/events?after=<id>` — security notices of the account. Needs the device token
/// of a managing device.
pub async fn events(
    State(st): State<Shared>,
    h: HeaderMap,
    query: Result<Query<EventsQuery>, QueryRejection>,
) -> Reply {
    let me = require_device(&st, &h)?;
    let Query(query) = query.map_err(|_| ApiError::InvalidRequest)?;
    let after = query.after.unwrap_or(0).max(0);
    let conn = st.db.lock();
    managing_device(&conn, &st, &me)?;
    let entries: Vec<Value> = device_events::after(&conn, me.account_id, after)?
        .into_iter()
        .map(|e| json!({"id": e.id, "event": e.kind, "device": e.device, "at": e.at}))
        .collect();
    Ok(Json(json!({ "events": entries })))
}
