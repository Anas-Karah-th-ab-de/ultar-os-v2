//! Ed25519 publisher keys and the set of trusted public keys.
//!
//! Thin wrappers around `ed25519-dalek`; no cryptography is implemented here.

use crate::error::Coded;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use std::fmt;

/// Lower-case hexadecimal SHA-256 of `data`.
pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// Why a key could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// The key is not valid hexadecimal.
    BadHex,
    /// The key is not exactly 32 bytes long.
    BadLength,
    /// The bytes are not a valid Ed25519 public key.
    BadPoint,
}

impl Coded for KeyError {
    fn code(&self) -> &'static str {
        match self {
            Self::BadHex => "driverpkg.key.bad-hex",
            Self::BadLength => "driverpkg.key.bad-length",
            Self::BadPoint => "driverpkg.key.bad-point",
        }
    }
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for KeyError {}

fn decode_32(hex_text: &str) -> Result<[u8; 32], KeyError> {
    let bytes = hex::decode(hex_text.trim()).map_err(|_| KeyError::BadHex)?;
    bytes.as_slice().try_into().map_err(|_| KeyError::BadLength)
}

/// A publisher's private signing key. Keep it offline; `Debug` never prints it.
#[derive(Clone)]
pub struct PrivateKey(SigningKey);

impl PrivateKey {
    /// Generates a fresh key from the operating system's random source.
    pub fn generate() -> Self {
        Self(SigningKey::generate(&mut rand::rngs::OsRng))
    }

    /// Reads a private key from 64 hexadecimal digits.
    pub fn from_hex(hex_text: &str) -> Result<Self, KeyError> {
        Ok(Self(SigningKey::from_bytes(&decode_32(hex_text)?)))
    }

    /// The private key as hexadecimal text (secret!).
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }

    /// The matching public key.
    pub fn public(&self) -> PublicKey {
        PublicKey(self.0.verifying_key())
    }

    /// Signs `message` and returns the 64-byte signature.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.0.sign(message).to_bytes()
    }
}

impl fmt::Debug for PrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PrivateKey(key_id={})", self.public().key_id())
    }
}

/// A publisher's public verification key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey(VerifyingKey);

impl PublicKey {
    /// Reads a public key from 64 hexadecimal digits.
    pub fn from_hex(hex_text: &str) -> Result<Self, KeyError> {
        VerifyingKey::from_bytes(&decode_32(hex_text)?)
            .map(Self)
            .map_err(|_| KeyError::BadPoint)
    }

    /// The public key as hexadecimal text.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }

    /// Short identifier (first 16 hex digits of the key's SHA-256) used to find the key for a signature.
    pub fn key_id(&self) -> String {
        sha256_hex(&self.0.to_bytes())[..16].to_string()
    }

    /// Strictly verifies a 64-byte signature over `message`.
    pub fn verify(&self, message: &[u8], signature: &[u8; 64]) -> bool {
        self.0
            .verify_strict(message, &Signature::from_bytes(signature))
            .is_ok()
    }
}

/// The set of public keys a device trusts for driver packages and depot indexes.
#[derive(Debug, Clone, Default)]
pub struct TrustedKeys {
    keys: Vec<PublicKey>,
}

impl TrustedKeys {
    /// Creates an empty set (which rejects everything).
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses a key file: one hexadecimal public key per line; blank lines and `#` comments are skipped.
    pub fn parse(text: &str) -> Result<Self, KeyError> {
        let mut set = Self::new();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or_default().trim();
            if !line.is_empty() {
                set.insert(PublicKey::from_hex(line)?);
            }
        }
        Ok(set)
    }

    /// Adds a key; returns `false` when it was already present.
    pub fn insert(&mut self, key: PublicKey) -> bool {
        if self.keys.contains(&key) {
            return false;
        }
        self.keys.push(key);
        true
    }

    /// Looks a key up by its [`PublicKey::key_id`].
    pub fn find(&self, key_id: &str) -> Option<&PublicKey> {
        self.keys.iter().find(|k| k.key_id() == key_id)
    }

    /// Number of trusted keys.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Returns `true` when no key is trusted.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

impl From<PublicKey> for TrustedKeys {
    fn from(key: PublicKey) -> Self {
        Self { keys: vec![key] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip_and_signing() {
        let key = PrivateKey::generate();
        let again = PrivateKey::from_hex(&key.to_hex()).unwrap();
        assert_eq!(key.public(), again.public());
        let public = PublicKey::from_hex(&key.public().to_hex()).unwrap();
        assert_eq!(public.key_id().len(), 16);
        let sig = key.sign(b"message");
        assert!(public.verify(b"message", &sig));
        assert!(!public.verify(b"other", &sig));
        assert!(!PrivateKey::generate().public().verify(b"message", &sig));
        assert!(!format!("{key:?}").contains(&key.to_hex()));
    }

    #[test]
    fn rejects_bad_keys() {
        assert_eq!(PrivateKey::from_hex("zz").unwrap_err(), KeyError::BadHex);
        assert_eq!(
            PrivateKey::from_hex("abcd").unwrap_err(),
            KeyError::BadLength
        );
        assert_eq!(PublicKey::from_hex("nothex").unwrap_err(), KeyError::BadHex);
        assert_eq!(
            PublicKey::from_hex(&"00".repeat(31)).unwrap_err(),
            KeyError::BadLength
        );
        // y = 2 is not on the curve.
        let off_curve = format!("02{}", "00".repeat(31));
        assert_eq!(
            PublicKey::from_hex(&off_curve).unwrap_err(),
            KeyError::BadPoint
        );
        assert_eq!(KeyError::BadPoint.to_string(), "driverpkg.key.bad-point");
    }

    #[test]
    fn trusted_set() {
        let a = PrivateKey::generate().public();
        let b = PrivateKey::generate().public();
        let text = format!(
            "# publisher keys\n{}\n\n  {}  # second\n{}\n",
            a.to_hex(),
            b.to_hex(),
            a.to_hex()
        );
        let set = TrustedKeys::parse(&text).unwrap();
        assert_eq!(set.len(), 2);
        assert!(!set.is_empty());
        assert_eq!(set.find(&a.key_id()), Some(&a));
        assert_eq!(set.find("0000000000000000"), None);
        assert!(TrustedKeys::new().is_empty());
        assert_eq!(TrustedKeys::parse("garbage").unwrap_err(), KeyError::BadHex);
        let mut single = TrustedKeys::from(a.clone());
        assert!(!single.insert(a));
        assert!(single.insert(b));
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
