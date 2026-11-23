//! Device tokens: bearer tokens bound to one device key.
//!
//! A device token is issued only after the device signed a fresh challenge. Its body is
//! `d1:<account>:<device id>:<expiry>:<account epoch>:<device epoch>`, authenticated with
//! HMAC-SHA256 under a key derived from the server's token key (so a device token can never be
//! taken for an account token and the other way round). The server compares both epochs and the
//! device's state with the database on every use ([`crate::entitlements::device_of_token`]):
//! removing the device or changing the password ends the token at once.

use crate::identity::is_device_id;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Lifetime of a device token in seconds (24 hours; a device renews it with its key).
pub const DEVICE_TOKEN_TTL_SECS: i64 = 86_400;

const KEY_LABEL: &[u8] = b"ultar-device-token-v1";
const VERSION: &str = "d1";
const MAX_TOKEN_LEN: usize = 256;

/// What a device token says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceClaims {
    /// Account of the device.
    pub account_id: i64,
    /// Key id of the device the token is bound to.
    pub device_id: String,
    /// Expiry time (Unix seconds).
    pub expires: i64,
    /// Token epoch of the account at issue time.
    pub account_epoch: i64,
    /// Token epoch of the device at issue time.
    pub device_epoch: i64,
}

fn mac(secret: &[u8; 32], body: &[u8]) -> Option<HmacSha256> {
    let mut derive = <HmacSha256 as Mac>::new_from_slice(secret).ok()?;
    derive.update(KEY_LABEL);
    let key = derive.finalize().into_bytes();
    let mut mac = <HmacSha256 as Mac>::new_from_slice(&key).ok()?;
    mac.update(body);
    Some(mac)
}

/// Issues a token for `claims`. `None` only if the claims cannot be encoded (never for ids made
/// by this server).
pub fn issue(secret: &[u8; 32], claims: &DeviceClaims) -> Option<String> {
    if !is_device_id(&claims.device_id) {
        return None;
    }
    let body = format!(
        "{VERSION}:{}:{}:{}:{}:{}",
        claims.account_id,
        claims.device_id,
        claims.expires,
        claims.account_epoch,
        claims.device_epoch
    );
    let sig = mac(secret, body.as_bytes())?.finalize().into_bytes();
    Some(format!("{}.{}", B64.encode(body), B64.encode(sig)))
}

/// Verifies signature (constant-time) and expiry. The caller must still compare the epochs and
/// the device's state with the database.
pub fn verify(secret: &[u8; 32], token: &str, now: i64) -> Option<DeviceClaims> {
    if token.len() > MAX_TOKEN_LEN {
        return None;
    }
    let (b, s) = token.split_once('.')?;
    let body = B64.decode(b).ok()?;
    let sig = B64.decode(s).ok()?;
    mac(secret, &body)?.verify_slice(&sig).ok()?;
    let body = String::from_utf8(body).ok()?;
    let fields: Vec<&str> = body.split(':').collect();
    let [version, account, device, expires, account_epoch, device_epoch] = fields.as_slice() else {
        return None;
    };
    if *version != VERSION || !is_device_id(device) {
        return None;
    }
    let claims = DeviceClaims {
        account_id: account.parse().ok()?,
        device_id: (*device).to_string(),
        expires: expires.parse().ok()?,
        account_epoch: account_epoch.parse().ok()?,
        device_epoch: device_epoch.parse().ok()?,
    };
    (now <= claims.expires).then_some(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7; 32];

    fn claims() -> DeviceClaims {
        DeviceClaims {
            account_id: 4,
            device_id: "a".repeat(32),
            expires: 1000,
            account_epoch: 2,
            device_epoch: 5,
        }
    }

    #[test]
    fn round_trip_and_expiry() {
        let token = issue(&KEY, &claims()).unwrap();
        assert!(token.len() <= MAX_TOKEN_LEN);
        assert_eq!(verify(&KEY, &token, 1000), Some(claims()));
        assert_eq!(verify(&KEY, &token, 1001), None, "expired");
        assert_eq!(verify(&[8; 32], &token, 0), None, "other key");
    }

    #[test]
    fn tampering_and_malformed_tokens_are_rejected() {
        let token = issue(&KEY, &claims()).unwrap();
        let (body, sig) = token.split_once('.').unwrap();
        let other = issue(
            &KEY,
            &DeviceClaims {
                device_id: "b".repeat(32),
                ..claims()
            },
        )
        .unwrap();
        let other_body = other.split_once('.').unwrap().0;
        assert_eq!(verify(&KEY, &format!("{other_body}.{sig}"), 0), None);
        let mut raw = B64.decode(sig).unwrap();
        raw[3] ^= 0x40;
        assert_eq!(
            verify(&KEY, &format!("{body}.{}", B64.encode(&raw)), 0),
            None
        );
        for bad in ["", "x", "a.b", &"a".repeat(300)] {
            assert_eq!(verify(&KEY, bad, 0), None, "{bad}");
        }
        let bad_id = DeviceClaims {
            device_id: "legacy:x".into(),
            ..claims()
        };
        assert_eq!(issue(&KEY, &bad_id), None);
    }

    #[test]
    fn account_and_device_tokens_are_never_interchangeable() {
        let device = issue(&KEY, &claims()).unwrap();
        assert_eq!(crate::auth::verify_token(&KEY, &device), None);
        let account = crate::auth::issue_token_epoch(&KEY, 4, 60, 2).unwrap();
        assert_eq!(verify(&KEY, &account, 0), None);
        // Even a correctly shaped device body signed with the account-token key is refused.
        let body = format!("d1:4:{}:99999999999:0:0", "a".repeat(32));
        let mut m = <HmacSha256 as Mac>::new_from_slice(&KEY).unwrap();
        m.update(body.as_bytes());
        let forged = format!(
            "{}.{}",
            B64.encode(&body),
            B64.encode(m.finalize().into_bytes())
        );
        assert_eq!(verify(&KEY, &forged, 0), None);
    }
}
