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

/// Token = base64(account_id:exp) + "." + base64(hmac). `None` only if the key is unusable.
pub fn issue_token(secret: &[u8], account_id: i64, ttl_secs: i64) -> Option<String> {
    let exp = crate::db::now() + ttl_secs;
    let body = format!("{account_id}:{exp}");
    let sig = mac_for(secret, body.as_bytes())?.finalize().into_bytes();
    Some(format!("{}.{}", B64.encode(body), B64.encode(sig)))
}

/// Verifies a token and returns its account id. The signature comparison is constant-time
/// (`verify_slice`), so response timing does not reveal how many signature bytes matched.
pub fn verify_token(secret: &[u8], token: &str) -> Option<i64> {
    verify_token_expiry(secret, token).map(|(id, _)| id)
}

/// Like [`verify_token`], and also returns the expiry time the token carries (so the caller can tell
/// when it was issued).
pub fn verify_token_expiry(secret: &[u8], token: &str) -> Option<(i64, i64)> {
    if token.len() > 256 {
        return None;
    }
    let (b, s) = token.split_once('.')?;
    let body = B64.decode(b).ok()?;
    let sig = B64.decode(s).ok()?;
    mac_for(secret, &body)?.verify_slice(&sig).ok()?;
    let body = String::from_utf8(body).ok()?;
    let (id, exp) = body.split_once(':')?;
    let exp: i64 = exp.parse().ok()?;
    if crate::db::now() > exp {
        return None;
    }
    Some((id.parse().ok()?, exp))
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
}
