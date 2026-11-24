//! Argon2id hashes in the PHC string format, made and checked by the `argon2` crate.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, SaltString};
use argon2::{Algorithm, Argon2, Params, PasswordVerifier, Version};
use std::fmt;

/// Memory cost in KiB of the shipped parameters (OWASP: Argon2id, 19 MiB, 2 passes).
pub const MEMORY_KIB: u32 = 19_456;
/// Passes of the shipped parameters.
pub const PASSES: u32 = 2;

/// The cost of new hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cost {
    /// Memory in KiB.
    pub memory_kib: u32,
    /// Passes.
    pub passes: u32,
}

impl Cost {
    /// The shipped cost.
    pub const STANDARD: Cost = Cost {
        memory_kib: MEMORY_KIB,
        passes: PASSES,
    };
    /// A cheap cost for tests only (never for stored accounts of a device).
    pub const TEST: Cost = Cost {
        memory_kib: 64,
        passes: 1,
    };
}

/// Why hashing failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashError {
    /// The parameters or the secret were refused by the hasher.
    Refused,
}

impl fmt::Display for HashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("argon2 refused the input")
    }
}

impl std::error::Error for HashError {}

fn hasher(cost: Cost) -> Result<Argon2<'static>, HashError> {
    let params =
        Params::new(cost.memory_kib, cost.passes, 1, None).map_err(|_| HashError::Refused)?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// Hashes `secret` with a fresh random salt; the PHC string carries the parameters.
pub fn make(secret: &[u8], cost: Cost) -> Result<String, HashError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = hasher(cost)?
        .hash_password(secret, &salt)
        .map_err(|_| HashError::Refused)?;
    Ok(hash.to_string())
}

/// True when `secret` matches `phc`; a malformed hash or another algorithm than
/// Argon2id never matches.
pub fn check(secret: &[u8], phc: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(phc) else {
        return false;
    };
    if parsed.algorithm.as_str() != "argon2id" {
        return false;
    }
    Argon2::default().verify_password(secret, &parsed).is_ok()
}

/// A well-formed hash characters check (the store format allows nothing else).
pub fn looks_like_phc(text: &str) -> bool {
    text.starts_with("$argon2id$")
        && text.len() <= 256
        && text.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '$' | '=' | ',' | '+' | '/' | '.' | '-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_check_and_differ_by_salt() {
        let a = make(b"geheim", Cost::TEST).unwrap();
        let b = make(b"geheim", Cost::TEST).unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with("$argon2id$v=19$m=64,t=1,p=1$"));
        assert!(looks_like_phc(&a));
        assert!(check(b"geheim", &a) && check(b"geheim", &b));
        assert!(!check(b"Geheim", &a) && !check(b"", &a));
        assert!(!check(b"geheim", "not a hash"));
        assert!(!looks_like_phc("$argon2id$ x") && !looks_like_phc("$argon2i$v=19"));
        let argon2i = "$argon2i$v=19$m=64,t=1,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaA";
        assert!(!check(b"x", argon2i));
        assert!(hasher(Cost {
            memory_kib: 1,
            passes: 0
        })
        .is_err());
        assert!(!HashError::Refused.to_string().is_empty());
    }

    #[test]
    fn the_shipped_cost_is_argon2id_19_mib_2_passes() {
        let h = make(b"pw", Cost::STANDARD).unwrap();
        assert!(h.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"), "{h}");
        assert!(check(b"pw", &h));
    }
}
