//! Device identity calls of the cloud client (`docs/DEVICE-IDENTITY.md` §5): registration with
//! proof of possession, device tokens, the device list, rename, remove, replace, recovery of a
//! lost device, the upgrade of an earlier registration, password change.
//!
//! Before anything is signed the client checks that the challenge names the server it is
//! configured for; a challenge relayed from another server is refused ([`CloudError::ServerIdentityMismatch`]).

use crate::client::Client;
use crate::error::CloudError;
use crate::identity::{self, Purpose, Transcript, CHALLENGE_BYTES, PUBLIC_KEY_BYTES};
use crate::keystore::KeyStore;
use serde::Deserialize;
use serde_json::{json, Value};

/// A challenge from the server, checked to be meant for this server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    /// Handle to send back.
    pub id: String,
    /// The bytes that enter the transcript.
    pub bytes: [u8; CHALLENGE_BYTES],
    /// Account the challenge is bound to.
    pub account_id: i64,
    /// Server name the signature will carry.
    pub server: String,
}

/// The answer to a registration, session, upgrade or move.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Registration {
    /// Key id of the device.
    pub device_id: String,
    /// Its Ultar number.
    pub number: String,
    /// Account of the device.
    pub account_id: i64,
    /// Device token (short-lived; renewed with the key).
    pub token: String,
    /// Lifetime of the token in seconds.
    pub expires_in: u64,
    /// True when this device may manage the account's other devices (rename, remove, approve,
    /// read notices, cancel a recovery). A device added to an account that already had devices
    /// gets that right by approval from one of them, or after the waiting time.
    #[serde(default)]
    pub manage: bool,
    /// Seconds until it may without approval (0 = it may).
    #[serde(default)]
    pub manage_in: i64,
    /// A replacement of one of the account's devices that is waiting to be finished.
    #[serde(default)]
    pub recovery: Option<RecoveryState>,
    /// Other devices of the account that were added and may not manage devices yet.
    #[serde(default)]
    pub unapproved: Vec<AddedDevice>,
}

/// A device that was added to the account and may manage its devices only after approval or
/// when its waiting time is over. Shown to the user until then.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AddedDevice {
    /// Its device id.
    pub device: String,
    /// Seconds until it may manage the account's devices without approval.
    pub manage_in: i64,
}

/// A pending replacement of a lost device: somebody with the account password asked to move a
/// number to a new device. Shown to the user until it is over or cancelled.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RecoveryState {
    /// Id of the device whose number is about to move.
    pub old_device: String,
    /// Seconds until the replacement can be finished (0 = now).
    pub eligible_in: i64,
}

/// The devices of an account and what is pending for it.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AccountDevices {
    /// The devices in use.
    pub devices: Vec<DeviceEntry>,
    /// Most devices the account may have.
    #[serde(default)]
    pub limit: u32,
    /// The pending replacement of a lost device, if any.
    #[serde(default)]
    pub recovery: Option<RecoveryState>,
}

/// One device of the account.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DeviceEntry {
    /// Key id, or `legacy:<id>` for a registration from before device keys.
    pub device_id: String,
    /// Name given by the user.
    pub name: String,
    /// Number, if the device has one.
    pub number: Option<String>,
    /// `active` or `legacy`.
    pub state: String,
    /// Day of registration (Unix seconds).
    pub added: i64,
    /// Day of last contact (Unix seconds, 0 = unknown).
    pub last_seen: i64,
    /// True for the device whose token made the request.
    pub current: bool,
    /// True when the device may manage the account's other devices.
    #[serde(default)]
    pub manage: bool,
    /// Its public key (base64url; absent for registrations from before device keys).
    #[serde(default)]
    pub public_key: Option<String>,
}

/// A security notice of the account.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DeviceEvent {
    /// Increasing id.
    pub id: i64,
    /// What happened (`device_added`, `recovery_started` …).
    pub event: String,
    /// First characters of the device id concerned.
    pub device: String,
    /// Time (Unix seconds).
    pub at: i64,
}

/// What a new device shows to the old one to ask for its number (QR code, cable, any channel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceRequest {
    /// The server's challenge for this replacement.
    pub challenge: Challenge,
    /// Public key of the new device.
    pub new_key: [u8; PUBLIC_KEY_BYTES],
}

/// The old device's signed consent to a [`ReplaceRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceGrant {
    /// Key id of the old device.
    pub old_device_id: String,
    /// Public key of the old device.
    pub old_key: [u8; PUBLIC_KEY_BYTES],
    /// Signature of the old key over the replacement.
    pub signature: String,
}

fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, CloudError> {
    serde_json::from_value(value).map_err(|_| CloudError::InvalidResponse)
}

/// Signs the transcript of `purpose` for `challenge` with `key`.
pub(crate) fn sign(
    key: &dyn KeyStore,
    purpose: Purpose,
    challenge: &Challenge,
    extra: &[u8],
) -> Result<String, CloudError> {
    let public = key.public_key()?;
    let transcript = Transcript {
        purpose,
        server: &challenge.server,
        account_id: challenge.account_id,
        challenge: &challenge.bytes,
        subject_key: &public,
        extra,
    };
    let bytes = transcript.bytes().ok_or(CloudError::InvalidResponse)?;
    Ok(identity::encode(&key.sign(&bytes)?))
}

/// The old device's part of a replacement: it signs "give my number to this new key" for the
/// server and account it is registered with (`server`, `account_id` from its own stored state).
/// A request for another server or account is refused and nothing is signed.
pub fn authorize_replacement(
    old_key: &dyn KeyStore,
    server: &str,
    account_id: i64,
    request: &ReplaceRequest,
) -> Result<ReplaceGrant, CloudError> {
    if request.challenge.server != server || request.challenge.account_id != account_id {
        return Err(CloudError::ServerIdentityMismatch);
    }
    let old_public = old_key.public_key()?;
    Ok(ReplaceGrant {
        old_device_id: identity::device_id(&old_public),
        old_key: old_public,
        signature: sign(
            old_key,
            Purpose::ReplaceOld,
            &request.challenge,
            &request.new_key,
        )?,
    })
}

impl Client {
    /// The server's name as it enters signatures; an address without one cannot be signed for.
    pub fn server_name(&self) -> Result<String, CloudError> {
        identity::server_name(self.base_url()).ok_or(CloudError::ServerIdentityMismatch)
    }

    /// Requests a challenge and checks that it is meant for the configured server.
    fn device_challenge(&self, purpose: &str, key: &dyn KeyStore) -> Result<Challenge, CloudError> {
        let expected = self.server_name()?;
        let body = json!({"purpose": purpose, "public_key": identity::encode(&key.public_key()?)});
        let v = self.post("/v1/device/challenge", &body)?;
        let text = |field: &str| v.get(field).and_then(Value::as_str);
        let challenge = Challenge {
            id: text("challenge_id")
                .ok_or(CloudError::InvalidResponse)?
                .to_string(),
            bytes: text("challenge")
                .and_then(identity::decode_fixed)
                .ok_or(CloudError::InvalidResponse)?,
            account_id: v
                .get("account_id")
                .and_then(Value::as_i64)
                .ok_or(CloudError::InvalidResponse)?,
            server: text("server")
                .ok_or(CloudError::InvalidResponse)?
                .to_string(),
        };
        if challenge.server != expected {
            return Err(CloudError::ServerIdentityMismatch);
        }
        Ok(challenge)
    }

    /// Registers the device key (proof of possession) and returns id, number and a device token.
    /// Needs an account. Registering the same key again returns the same number.
    pub fn register_device(
        &self,
        key: &dyn KeyStore,
        name: &str,
    ) -> Result<Registration, CloudError> {
        self.require_token()?;
        let c = self.device_challenge("register", key)?;
        let body = json!({
            "challenge_id": c.id,
            "signature": sign(key, Purpose::Register, &c, b"")?,
            "name": name,
        });
        parse(self.post("/v1/device/register", &body)?)
    }

    /// Gets a device token for the registered key. Needs no account token.
    pub fn device_session(&self, key: &dyn KeyStore) -> Result<Registration, CloudError> {
        let c = self.device_challenge("session", key)?;
        let body =
            json!({"challenge_id": c.id, "signature": sign(key, Purpose::Session, &c, b"")?});
        parse(self.post("/v1/device/session", &body)?)
    }

    /// A client that acts with a fresh device token of `key`.
    pub fn as_device(&self, key: &dyn KeyStore) -> Result<Client, CloudError> {
        let session = self.device_session(key)?;
        Ok(Client::new(self.base_url(), Some(session.token)))
    }

    /// Takes over a registration made before device keys (`legacy_id` = the id this device used
    /// then). Needs the account token the device already had.
    pub fn upgrade_legacy_device(
        &self,
        key: &dyn KeyStore,
        legacy_id: &str,
        name: &str,
    ) -> Result<Registration, CloudError> {
        self.require_token()?;
        let c = self.device_challenge("upgrade", key)?;
        let body = json!({
            "legacy_device_id": legacy_id,
            "challenge_id": c.id,
            "signature": sign(key, Purpose::Upgrade, &c, legacy_id.as_bytes())?,
            "name": name,
        });
        parse(self.post("/v1/device/upgrade", &body)?)
    }

    /// The devices of the signed-in account.
    pub fn devices(&self) -> Result<Vec<DeviceEntry>, CloudError> {
        Ok(self.account_devices()?.devices)
    }

    /// The devices of the signed-in account together with a pending replacement, if any.
    pub fn account_devices(&self) -> Result<AccountDevices, CloudError> {
        self.require_token()?;
        parse(self.get_value("/v1/devices")?)
    }

    /// This (managing) device gives the added device `device_id` its management rights at once.
    /// The key of that device is taken from the account's device list and must hash to the id
    /// the user confirmed; this device then signs it.
    pub fn approve_device(&self, key: &dyn KeyStore, device_id: &str) -> Result<(), CloudError> {
        let as_device = self.as_device(key)?;
        let target_key = as_device
            .devices()?
            .into_iter()
            .find(|d| d.device_id == device_id)
            .and_then(|d| d.public_key)
            .and_then(|k| identity::decode_fixed::<PUBLIC_KEY_BYTES>(&k))
            .filter(|k| identity::device_id(k) == device_id)
            .ok_or(CloudError::DeviceUnknown)?;
        let c = as_device.device_challenge("approve", key)?;
        let body = json!({
            "device_id": device_id,
            "challenge_id": c.id,
            "signature": sign(key, Purpose::Approve, &c, &target_key)?,
        });
        as_device.post("/v1/device/approve", &body).map(|_| ())
    }

    /// Renames a device of the account. Authorised by this device's key.
    pub fn rename_device(
        &self,
        key: &dyn KeyStore,
        device_id: &str,
        name: &str,
    ) -> Result<(), CloudError> {
        let body = json!({"device_id": device_id, "name": name});
        self.as_device(key)?
            .post("/v1/device/rename", &body)
            .map(|_| ())
    }

    /// Removes a device of the account (its number is retired). Authorised by this device's key.
    pub fn remove_device(&self, key: &dyn KeyStore, device_id: &str) -> Result<(), CloudError> {
        let body = json!({ "device_id": device_id });
        self.as_device(key)?
            .post("/v1/device/remove", &body)
            .map(|_| ())
    }

    /// Security notices with an id above `after`, read with the device token this client holds.
    pub(crate) fn events_after(&self, after: i64) -> Result<Vec<DeviceEvent>, CloudError> {
        let v = self.get_value(&format!("/v1/device/events?after={after}"))?;
        parse(
            v.get("events")
                .cloned()
                .ok_or(CloudError::InvalidResponse)?,
        )
    }

    /// Security notices of the account with an id above `after`.
    pub fn device_events(
        &self,
        key: &dyn KeyStore,
        after: i64,
    ) -> Result<Vec<DeviceEvent>, CloudError> {
        self.as_device(key)?.events_after(after)
    }

    /// New device, step 1 of a replacement: asks the server for the challenge both devices sign.
    pub fn replace_begin(&self, new_key: &dyn KeyStore) -> Result<ReplaceRequest, CloudError> {
        self.require_token()?;
        Ok(ReplaceRequest {
            challenge: self.device_challenge("replace", new_key)?,
            new_key: new_key.public_key()?,
        })
    }

    /// New device, step 2: sends the old device's consent together with its own proof.
    pub fn replace_finish(
        &self,
        new_key: &dyn KeyStore,
        request: &ReplaceRequest,
        grant: &ReplaceGrant,
        name: &str,
    ) -> Result<Registration, CloudError> {
        self.require_token()?;
        let body = json!({
            "challenge_id": request.challenge.id,
            "old_device_id": grant.old_device_id,
            "old_signature": grant.signature,
            "new_signature": sign(new_key, Purpose::ReplaceNew, &request.challenge, &grant.old_key)?,
            "name": name,
        });
        parse(self.post("/v1/device/replace", &body)?)
    }

    /// Starts the replacement of a lost device: password, proof of the new key. Returns the
    /// waiting time in seconds. Every device of the account is told.
    pub fn recover_start(
        &self,
        new_key: &dyn KeyStore,
        password: &str,
        old_device_id: &str,
    ) -> Result<u64, CloudError> {
        self.require_token()?;
        let c = self.device_challenge("recover", new_key)?;
        let body = json!({
            "password": password,
            "old_device_id": old_device_id,
            "challenge_id": c.id,
            "signature": sign(new_key, Purpose::RecoverStart, &c, old_device_id.as_bytes())?,
        });
        self.post("/v1/device/recover/start", &body)?
            .get("eligible_in")
            .and_then(Value::as_u64)
            .ok_or(CloudError::InvalidResponse)
    }

    /// Finishes the replacement of a lost device after the waiting time.
    pub fn recover_finish(
        &self,
        new_key: &dyn KeyStore,
        old_device_id: &str,
        name: &str,
    ) -> Result<Registration, CloudError> {
        self.require_token()?;
        let c = self.device_challenge("recover", new_key)?;
        let body = json!({
            "old_device_id": old_device_id,
            "challenge_id": c.id,
            "signature": sign(new_key, Purpose::RecoverFinish, &c, old_device_id.as_bytes())?,
            "name": name,
        });
        parse(self.post("/v1/device/recover/finish", &body)?)
    }

    /// Cancels a pending replacement. Authorised by this device's key (a managing device).
    pub fn recover_cancel(&self, key: &dyn KeyStore) -> Result<(), CloudError> {
        self.as_device(key)?
            .post("/v1/device/recover/cancel", &json!({}))
            .map(|_| ())
    }

    /// Changes the account password; returns the new account token. All earlier tokens of the
    /// account stop working; devices stay registered.
    pub fn change_password(
        &self,
        old_password: &str,
        new_password: &str,
    ) -> Result<String, CloudError> {
        self.require_token()?;
        let body = json!({"old_password": old_password, "new_password": new_password});
        self.post("/v1/account/password", &body)?
            .get("token")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or(CloudError::InvalidResponse)
    }
}

#[cfg(test)]
#[path = "client_device_tests.rs"]
mod tests;
