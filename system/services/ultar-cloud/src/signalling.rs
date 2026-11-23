//! Sign-in at the signalling socket (`docs/DEVICE-IDENTITY.md` §5.4) from the device's side.
//!
//! The device proves its key by signing the server's challenge. It signs only for the server it
//! is **connected to**: [`DeviceHome::signal_proof`](crate::device::DeviceHome::signal_proof)
//! hands out a proof for an address only when that address is the server the device is
//! registered with, so a device that is pointed at another server signs nothing that the real
//! server would accept.

use crate::client_device::{sign, Challenge};
use crate::error::CloudError;
use crate::identity::{self, Purpose};
use crate::keystore::{FileKeyStore, KeyStore};
use serde_json::{json, Value};

/// First frame of the signalling socket: names the device.
pub fn signal_hello(key: &dyn KeyStore) -> Result<String, CloudError> {
    let device_id = identity::device_id(&key.public_key()?);
    Ok(json!({"type": "hello", "device_id": device_id}).to_string())
}

/// Answer to the server's `challenge` frame on the signalling socket. `server` and `account_id`
/// are what this device is registered with; a challenge that names anything else is refused and
/// nothing is signed.
pub fn signal_auth(
    key: &dyn KeyStore,
    server: &str,
    account_id: i64,
    challenge_frame: &str,
) -> Result<String, CloudError> {
    let v: Value =
        serde_json::from_str(challenge_frame).map_err(|_| CloudError::InvalidResponse)?;
    let text = |field: &str| v.get(field).and_then(Value::as_str);
    if text("type") != Some("challenge") {
        return Err(CloudError::InvalidResponse);
    }
    let challenge = Challenge {
        id: String::new(),
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
    if challenge.server != server || challenge.account_id != account_id {
        return Err(CloudError::ServerIdentityMismatch);
    }
    let signature = sign(key, Purpose::Signal, &challenge, b"")?;
    Ok(json!({"type": "auth", "signature": signature}).to_string())
}

/// The device's proof for the signalling socket of one server. Obtained from
/// [`DeviceHome::signal_proof`](crate::device::DeviceHome::signal_proof), which checks that the
/// server is the one the device is registered with.
#[derive(Debug)]
pub struct SignalProof {
    key: FileKeyStore,
    server: String,
    account_id: i64,
}

impl SignalProof {
    pub(crate) fn new(key: FileKeyStore, server: String, account_id: i64) -> Self {
        Self {
            key,
            server,
            account_id,
        }
    }

    /// Name of the server this proof is for.
    pub fn server(&self) -> &str {
        &self.server
    }

    /// First frame: names the device.
    pub fn hello(&self) -> Result<String, CloudError> {
        signal_hello(&self.key)
    }

    /// Signed answer to the server's challenge frame; refused when the challenge names another
    /// server or account.
    pub fn answer(&self, challenge_frame: &str) -> Result<String, CloudError> {
        signal_auth(&self.key, &self.server, self.account_id, challenge_frame)
    }
}
