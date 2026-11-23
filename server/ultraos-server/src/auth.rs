//! Accounts: passwords (Argon2id) and signed session tokens (HMAC-SHA256).
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::fmt;

type HmacSha256 = Hmac<Sha256>;

/// Password hashing failed (never caused by the password's content in practice).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashError;

impl fmt::Display for HashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("password hashing failed")
    }
}

impl std::error::Error for HashError {}

/// Hashes a password with Argon2id and a fresh salt (PHC string).
pub fn hash_password(pw: &str) -> Result<String, HashError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| HashError)
}

/// Checks a password against a stored PHC string.
pub fn verify_password(pw: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(h) => Argon2::default().verify_password(pw.as_bytes(), &h).is_ok(),
        Err(_) => false,
    }
}

fn mac_for(secret: &[u8], body: &[u8]) -> Option<HmacSha256> {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(secret).ok()?;
    mac.update(body);
    Some(mac)
}

/// What a verified account token says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountClaims {
    /// The account the token stands for.
    pub account_id: i64,
    /// Expiry time (Unix seconds).
    pub expires: i64,
    /// Token epoch of the account when the token was issued (0 for tokens without the field).
    pub epoch: i64,
}

/// Token = base64(account_id:exp) + "." + base64(hmac). `None` only if the key is unusable.
pub fn issue_token(secret: &[u8], account_id: i64, ttl_secs: i64) -> Option<String> {
    issue_token_epoch(secret, account_id, ttl_secs, 0)
}

/// Like [`issue_token`] for an account whose token epoch is `epoch`. The body is
/// `account_id:exp:epoch`; epoch 0 keeps the two-field form of the first server release, so
/// tokens issued before epochs existed and new tokens of an untouched account are the same.
pub fn issue_token_epoch(
    secret: &[u8],
    account_id: i64,
    ttl_secs: i64,
    epoch: i64,
) -> Option<String> {
    let exp = crate::db::now() + ttl_secs;
    let body = if epoch == 0 {
        format!("{account_id}:{exp}")
    } else {
        format!("{account_id}:{exp}:{epoch}")
    };
    let sig = mac_for(secret, body.as_bytes())?.finalize().into_bytes();
    Some(format!("{}.{}", B64.encode(body), B64.encode(sig)))
}

/// Verifies a token and returns its account id. The signature comparison is constant-time
/// (`verify_slice`), so response timing does not reveal how many signature bytes matched.
///
/// This checks signature and expiry only. Whether the token is still in force (account exists,
/// epoch current) is decided by [`crate::entitlements::account_of_token`].
pub fn verify_token(secret: &[u8], token: &str) -> Option<i64> {
    verify_token_claims(secret, token).map(|c| c.account_id)
}

/// Like [`verify_token`], and also returns the expiry time the token carries (so the caller can tell
/// when it was issued).
pub fn verify_token_expiry(secret: &[u8], token: &str) -> Option<(i64, i64)> {
    verify_token_claims(secret, token).map(|c| (c.account_id, c.expires))
}

/// Verifies signature and expiry of an account token and returns everything it carries.
pub fn verify_token_claims(secret: &[u8], token: &str) -> Option<AccountClaims> {
    if token.len() > 256 {
        return None;
    }
    let (b, s) = token.split_once('.')?;
    let body = B64.decode(b).ok()?;
    let sig = B64.decode(s).ok()?;
    mac_for(secret, &body)?.verify_slice(&sig).ok()?;
    let body = String::from_utf8(body).ok()?;
    let mut fields = body.split(':');
    let account_id: i64 = fields.next()?.parse().ok()?;
    let expires: i64 = fields.next()?.parse().ok()?;
    let epoch: i64 = match fields.next() {
        Some(text) => text.parse().ok()?,
        None => 0,
    };
    if fields.next().is_some() || crate::db::now() > expires {
        return None;
    }
    Some(AccountClaims {
        account_id,
        expires,
        epoch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7; 32];

    #[test]
    fn password_round_trip() {
        let phc = hash_password("correct horse").unwrap();
        assert!(phc.starts_with("$argon2id$"));
        assert!(verify_password("correct horse", &phc));
        assert!(!verify_password("wrong", &phc));
        assert!(!verify_password("correct horse", "not a phc string"));
        assert_eq!(HashError.to_string(), "password hashing failed");
    }

    #[test]
    fn token_round_trip_and_rejections() {
        let t = issue_token(&KEY, 42, 60).unwrap();
        assert_eq!(verify_token(&KEY, &t), Some(42));
        assert_eq!(verify_token(&[8; 32], &t), None, "other key");
        assert_eq!(
            verify_token(&KEY, &issue_token(&KEY, 42, -5).unwrap()),
            None,
            "expired"
        );
        assert_eq!(verify_token(&KEY, ""), None);
        assert_eq!(verify_token(&KEY, "no-dot"), None);
        assert_eq!(verify_token(&KEY, &"a".repeat(300)), None);
    }

    #[test]
    fn tampered_tokens_are_rejected() {
        let t = issue_token(&KEY, 42, 60).unwrap();
        let (body, sig) = t.split_once('.').unwrap();
        let forged_body = B64.encode(format!("43:{}", crate::db::now() + 60));
        assert_eq!(verify_token(&KEY, &format!("{forged_body}.{sig}")), None);
        let mut sig_bytes = B64.decode(sig).unwrap();
        sig_bytes[0] ^= 1;
        assert_eq!(
            verify_token(&KEY, &format!("{body}.{}", B64.encode(&sig_bytes))),
            None
        );
        sig_bytes.truncate(8);
        assert_eq!(
            verify_token(&KEY, &format!("{body}.{}", B64.encode(&sig_bytes))),
            None
        );
    }

    #[test]
    fn epochs_travel_in_the_token_and_epoch_zero_keeps_the_old_form() {
        let old = issue_token(&KEY, 42, 60).unwrap();
        let zero = issue_token_epoch(&KEY, 42, 60, 0).unwrap();
        let body = |t: &str| String::from_utf8(B64.decode(t.split_once('.').unwrap().0).unwrap());
        assert_eq!(body(&old).unwrap().matches(':').count(), 1);
        assert_eq!(body(&zero).unwrap().matches(':').count(), 1);
        assert_eq!(verify_token_claims(&KEY, &old).unwrap().epoch, 0);
        let three = issue_token_epoch(&KEY, 42, 60, 3).unwrap();
        let claims = verify_token_claims(&KEY, &three).unwrap();
        assert_eq!((claims.account_id, claims.epoch), (42, 3));
        assert_eq!(verify_token(&KEY, &three), Some(42));
        assert_eq!(
            verify_token_expiry(&KEY, &three),
            Some((42, claims.expires))
        );
    }

    #[test]
    fn bodies_with_other_shapes_are_rejected_even_when_correctly_signed() {
        let signed = |body: &str| {
            let sig = mac_for(&KEY, body.as_bytes())
                .unwrap()
                .finalize()
                .into_bytes();
            format!("{}.{}", B64.encode(body), B64.encode(sig))
        };
        let exp = crate::db::now() + 60;
        assert!(verify_token_claims(&KEY, &signed(&format!("1:{exp}:2"))).is_some());
        for body in [
            format!("1:{exp}:2:3"),
            format!("1:{exp}:x"),
            format!("d1:1:abc:{exp}:0:0"),
            format!("1:{exp}:"),
            "1".to_string(),
        ] {
            assert_eq!(verify_token_claims(&KEY, &signed(&body)), None, "{body}");
        }
    }
}
