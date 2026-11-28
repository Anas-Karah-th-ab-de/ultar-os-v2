//! Secrets the installer holds in memory (user password, encryption passphrase): never
//! printed, compared in constant time, wiped when dropped. The first start gets only an
//! Argon2id hash of the user password (PHC string), never the password.

use zeroize::Zeroizing;

/// Longest secret in bytes (the text fields' limit).
pub const MAX_SECRET: usize = 256;

/// A secret text.
#[derive(Clone, Default)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    /// A secret from typed text (cut at [`MAX_SECRET`] bytes on a character boundary).
    pub fn new(text: &str) -> Secret {
        let mut end = text.len().min(MAX_SECRET);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        Secret(Zeroizing::new(text[..end].to_owned()))
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The text, to put back into its own password field after the page was rebuilt.
    pub fn expose_for_field(&self) -> &str {
        &self.0
    }

    /// The text (for hashing only).
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl PartialEq for Secret {
    /// Constant time in the length of the longer secret.
    fn eq(&self, other: &Secret) -> bool {
        let (a, b) = (self.0.as_bytes(), other.0.as_bytes());
        let n = a.len().max(b.len());
        let mut diff = u8::from(a.len() != b.len());
        for i in 0..n {
            diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
        }
        std::hint::black_box(diff) == 0
    }
}

impl Eq for Secret {}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<hidden>")
    }
}

/// Why no hash was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashError;

/// Argon2id (default parameters of the `argon2` crate: 19 MiB, 2 passes) with a random
/// 16-byte salt, as a PHC string (`$argon2id$v=19$m=…`).
pub fn hash_password(password: &Secret) -> Result<String, HashError> {
    use argon2::password_hash::{PasswordHasher, SaltString};
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|_| HashError)?;
    let salt = SaltString::encode_b64(&salt).map_err(|_| HashError)?;
    argon2::Argon2::default()
        .hash_password(password.expose().as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| HashError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::password_hash::{PasswordHash, PasswordVerifier};

    #[test]
    fn secrets_compare_hide_and_hash() {
        let (a, b) = (Secret::new("correct horse"), Secret::new("correct horse"));
        assert_eq!(a, b);
        assert_ne!(a, Secret::new("correct hors"));
        assert_ne!(a, Secret::new("correct horsf"));
        assert_eq!(format!("{a:?}"), "<hidden>");
        assert_eq!(Secret::new(&"ع".repeat(200)).len(), 256);
        let h = hash_password(&a).unwrap();
        assert!(h.starts_with("$argon2id$v=19$"), "{h}");
        assert!(!h.contains("correct"));
        let parsed = PasswordHash::new(&h).unwrap();
        assert!(argon2::Argon2::default()
            .verify_password(b"correct horse", &parsed)
            .is_ok());
        assert!(argon2::Argon2::default()
            .verify_password(b"wrong", &parsed)
            .is_err());
        assert_ne!(h, hash_password(&a).unwrap(), "fresh salt each time");
    }
}
