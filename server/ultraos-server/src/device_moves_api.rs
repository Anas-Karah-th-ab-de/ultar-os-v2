//! HTTP endpoints that move a number to another key (`docs/DEVICE-IDENTITY.md` §5.6, §5.7):
//! replacement signed by the old device, and replacement of a lost device with password,
//! cool-down and notice. Same order as in [`crate::device_api`]: authenticate, parse, consume the
//! challenge, verify every signature, then one database transaction.

use crate::accounts::{password_matches, ApiJson};
use crate::challenge::ChallengeUse;
use crate::device_api::{
    managing_device, notice_prefix, proven, registered_reply, short, Proof, Reply, Shared,
};
use crate::device_moves;
use crate::devices::{self, MAX_DEVICE_ID_BYTES};
use crate::entitlements::{require_account, require_device};
use crate::error::ApiError;
use crate::identity::{self, Purpose, Transcript};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

/// Longest password accepted in a request body.
const MAX_PASSWORD_BYTES: usize = 256;

/// Body of the replacement authorised by the old device.
#[derive(Deserialize)]
pub struct ReplaceReq {
    challenge_id: String,
    old_device_id: String,
    old_signature: String,
    new_signature: String,
    #[serde(default)]
    name: String,
}

/// `POST /v1/device/replace` — moves the number of an active device to a new key. The old key
/// signs "replace me by this new key" and the new key proves possession, both over the same
/// challenge.
pub async fn replace(
    State(st): State<Shared>,
    h: HeaderMap,
    ApiJson(r): ApiJson<ReplaceReq>,
) -> Reply {
    let acc = require_account(&st, &h)?;
    let proof = Proof::parse(&r.challenge_id, &r.new_signature)?;
    let old_signature = identity::parse_signature(&r.old_signature)?;
    devices::clean_name(&r.name)?;
    if r.old_device_id.len() > MAX_DEVICE_ID_BYTES {
        return Err(ApiError::InvalidRequest);
    }
    // The old device's key is public; it is read first so that the new device's transcript can
    // name it. Everything is checked again inside the transaction of `replace`.
    let old = devices::in_use(&st.db.lock(), acc, &r.old_device_id)?;
    let old_key = old.public_key.ok_or(ApiError::DeviceUnknown)?;
    let bound = proven(
        &st,
        &proof,
        ChallengeUse::Replace,
        Some(acc),
        Purpose::ReplaceNew,
        &old_key,
    )?;
    let by_old = Transcript {
        purpose: Purpose::ReplaceOld,
        server: &st.devices.config.server_name,
        account_id: acc,
        challenge: &bound.challenge,
        subject_key: &old_key,
        extra: &bound.public_key,
    };
    by_old.verify(&old_signature)?;
    let now = st.devices.now_secs();
    let limits = st.devices.limits();
    let done = device_moves::replace(
        &mut st.db.lock(),
        &old,
        &bound.public_key,
        &r.name,
        now,
        limits,
    )?;
    st.hub.kick(&done.number);
    st.hub.notify_account(
        st.devices.now_secs(),
        acc,
        "device_replaced",
        &notice_prefix(&done.device_id),
    );
    tracing::info!(event = "device_replaced", device = %short(&done.device_id));
    registered_reply(&st, acc, &done)
}

/// Body of the start of a lost-device recovery.
#[derive(Deserialize)]
pub struct RecoverStartReq {
    password: String,
    old_device_id: String,
    challenge_id: String,
    signature: String,
}

/// `POST /v1/device/recover/start` — asks to move the number of a lost device to a new key. Needs
/// the account password and a proof of possession of the new key. Starts the cool-down and tells
/// every device of the account.
pub async fn recover_start(
    State(st): State<Shared>,
    h: HeaderMap,
    client: Option<axum::Extension<crate::guard::ClientAddr>>,
    ApiJson(r): ApiJson<RecoverStartReq>,
) -> Reply {
    let acc = require_account(&st, &h)?;
    let proof = Proof::parse(&r.challenge_id, &r.signature)?;
    if r.old_device_id.len() > MAX_DEVICE_ID_BYTES || r.password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError::InvalidRequest);
    }
    let asker = crate::accounts::asker_of(&st, &h, client.map(|c| c.0 .0));
    if !password_matches(&st, acc, asker, r.password).await? {
        return Err(ApiError::InvalidCredentials);
    }
    let extra = r.old_device_id.as_bytes();
    let bound = proven(
        &st,
        &proof,
        ChallengeUse::Recover,
        Some(acc),
        Purpose::RecoverStart,
        extra,
    )?;
    let now = st.devices.now_secs();
    let eligible_at = device_moves::recovery_start(
        &mut st.db.lock(),
        acc,
        &r.old_device_id,
        &bound.public_key,
        now,
        st.devices.config.recovery_cooldown_secs,
    )?;
    st.hub.notify_account(
        st.devices.now_secs(),
        acc,
        "recovery_started",
        &notice_prefix(&r.old_device_id),
    );
    tracing::info!(event = "recovery_started", device = %short(&r.old_device_id));
    Ok(Json(json!({ "eligible_in": (eligible_at - now).max(0) })))
}

/// `POST /v1/device/recover/cancel` — cancels the pending recovery of the account. Needs a device
/// token of a managing device of the account (the one reported lost included).
pub async fn recover_cancel(State(st): State<Shared>, h: HeaderMap) -> Reply {
    let me = require_device(&st, &h)?;
    {
        let mut conn = st.db.lock();
        managing_device(&conn, &st, &me)?;
        device_moves::recovery_cancel(&mut conn, me.account_id, st.devices.now_secs())?;
    }
    st.hub.notify_account(
        st.devices.now_secs(),
        me.account_id,
        "recovery_cancelled",
        "",
    );
    tracing::info!(event = "recovery_cancelled", device = %short(&me.device_id));
    Ok(Json(json!({ "cancelled": true })))
}

/// Body of the end of a lost-device recovery.
#[derive(Deserialize)]
pub struct RecoverFinishReq {
    old_device_id: String,
    challenge_id: String,
    signature: String,
    #[serde(default)]
    name: String,
}

/// `POST /v1/device/recover/finish` — completes a recovery after its cool-down, with a fresh
/// proof of possession of the key the recovery was started with.
pub async fn recover_finish(
    State(st): State<Shared>,
    h: HeaderMap,
    ApiJson(r): ApiJson<RecoverFinishReq>,
) -> Reply {
    let acc = require_account(&st, &h)?;
    let proof = Proof::parse(&r.challenge_id, &r.signature)?;
    devices::clean_name(&r.name)?;
    if r.old_device_id.len() > MAX_DEVICE_ID_BYTES {
        return Err(ApiError::InvalidRequest);
    }
    let extra = r.old_device_id.as_bytes();
    let bound = proven(
        &st,
        &proof,
        ChallengeUse::Recover,
        Some(acc),
        Purpose::RecoverFinish,
        extra,
    )?;
    let done = device_moves::recovery_finish(
        &mut st.db.lock(),
        acc,
        &r.old_device_id,
        &bound.public_key,
        &r.name,
        st.devices.now_secs(),
        st.devices.limits(),
    )?;
    st.hub.kick(&done.number);
    st.hub.notify_account(
        st.devices.now_secs(),
        acc,
        "recovery_finished",
        &notice_prefix(&done.device_id),
    );
    tracing::info!(event = "recovery_finished", device = %short(&done.device_id));
    registered_reply(&st, acc, &done)
}

/// Body of an approval.
#[derive(Deserialize)]
pub struct ApproveReq {
    device_id: String,
    challenge_id: String,
    signature: String,
}

/// `POST /v1/device/approve` — a managing device gives a device that was added to the account
/// its management rights at once. Needs the managing device's token **and** its signature over
/// the new device's key (challenge of purpose `approve` issued for the approver's key,
/// transcript `ultar-device-approve-v1`, extra = public key of the approved device): a stolen
/// device token alone approves nothing.
pub async fn approve(
    State(st): State<Shared>,
    h: HeaderMap,
    ApiJson(r): ApiJson<ApproveReq>,
) -> Reply {
    let me = require_device(&st, &h)?;
    let proof = Proof::parse(&r.challenge_id, &r.signature)?;
    if !identity::is_device_id(&r.device_id) {
        return Err(ApiError::InvalidRequest);
    }
    let acc = me.account_id;
    let target_key = devices::in_use(&st.db.lock(), acc, &r.device_id)?
        .public_key
        .ok_or(ApiError::DeviceUnknown)?;
    let bound = proven(
        &st,
        &proof,
        ChallengeUse::Approve,
        Some(acc),
        Purpose::Approve,
        &target_key,
    )?;
    // The signature must be the calling device's own.
    if identity::device_id(&bound.public_key) != me.device_id {
        return Err(ApiError::ChallengeMismatch);
    }
    let number = {
        let mut conn = st.db.lock();
        let approver = managing_device(&conn, &st, &me)?;
        devices::approve(
            &mut conn,
            &approver,
            &r.device_id,
            st.devices.now_secs(),
            st.devices.config.recovery_cooldown_secs,
        )?;
        let row = devices::in_use(&conn, acc, &r.device_id)?;
        devices::number_of(&conn, row.row)?
    };
    // A socket the approved device holds right now gets the notices from now on.
    if let Some(number) = &number {
        st.hub.approved(number);
    }
    st.hub.notify_account(
        st.devices.now_secs(),
        acc,
        "device_approved",
        &notice_prefix(&r.device_id),
    );
    tracing::info!(event = "device_approved", device = %short(&r.device_id));
    Ok(Json(json!({ "approved": true })))
}
