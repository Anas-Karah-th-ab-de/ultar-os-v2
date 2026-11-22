//! Konten, Passwörter (Argon2id) und signierte Sitzungstoken (HMAC-SHA256).
use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub fn hash_password(pw: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

pub fn verify_password(pw: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(h) => Argon2::default().verify_password(pw.as_bytes(), &h).is_ok(),
        Err(_) => false,
    }
}

/// Token = base64(account_id:exp) + "." + base64(hmac). Serverschlüssel aus Env/zufällig.
pub fn issue_token(secret: &[u8], account_id: i64, ttl_secs: i64) -> String {
    let exp = crate::db::now() + ttl_secs;
    let body = format!("{account_id}:{exp}");
    let mut mac = HmacSha256::new_from_slice(secret).expect("key");
    mac.update(body.as_bytes());
    let sig = mac.finalize().into_bytes();
    format!("{}.{}", B64.encode(body), B64.encode(sig))
}

pub fn verify_token(secret: &[u8], token: &str) -> Option<i64> {
    let (b, s) = token.split_once('.')?;
    let body = B64.decode(b).ok()?;
    let sig = B64.decode(s).ok()?;
    let mut mac = HmacSha256::new_from_slice(secret).ok()?;
    mac.update(&body);
    mac.verify_slice(&sig).ok()?;
    let body = String::from_utf8(body).ok()?;
    let (id, exp) = body.split_once(':')?;
    let exp: i64 = exp.parse().ok()?;
    if crate::db::now() > exp {
        return None;
    }
    id.parse().ok()
}
