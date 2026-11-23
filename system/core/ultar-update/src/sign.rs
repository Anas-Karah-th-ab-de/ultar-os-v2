//! Signed envelopes: `magic · u16 length · body · key id · Ed25519 signature`.
//!
//! The signature covers `domain + body`. Each document type has its own domain string, so a
//! signature made for one purpose (an app package, a key rotation) is never valid for another.
//! No cryptography is implemented here; `ed25519-dalek` does the work (strict verification).

use crate::error::{Malformed, UpdateError};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

/// Magic of a signed update manifest.
pub const MANIFEST_MAGIC: [u8; 4] = *b"UUM1";
/// Domain of a manifest signature.
pub const MANIFEST_DOMAIN: &[u8] = b"ultar-update-v1\n";
/// Magic of a signed key rotation.
pub const KEYS_MAGIC: [u8; 4] = *b"UUK1";
/// Domain of a rotation signature.
pub const KEYS_DOMAIN: &[u8] = b"ultar-update-keys-v1\n";
/// Bytes around the body: magic, length, key id, signature.
pub const ENVELOPE_OVERHEAD: usize = 4 + 2 + 8 + 64;

/// A raw Ed25519 public key.
pub type PublicKey = [u8; 32];

/// Short identifier of a public key: the first 8 bytes of its SHA-256.
pub fn key_id(key: &PublicKey) -> [u8; 8] {
    let digest = Sha256::digest(key);
    let mut id = [0u8; 8];
    id.copy_from_slice(&digest[..8]);
    id
}

/// A parsed, not yet verified envelope borrowing from the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Envelope<'a> {
    /// The signed body.
    pub body: &'a [u8],
    /// Identifier of the key that signed.
    pub key_id: [u8; 8],
    /// The signature.
    pub signature: [u8; 64],
    /// Total bytes the envelope occupies in the input.
    pub len: usize,
}

/// Reads one envelope from the start of `bytes` (more data may follow; see `len`).
/// `max_body` bounds the body before anything is copied.
pub fn open<'a>(
    magic: &[u8; 4],
    bytes: &'a [u8],
    max_body: usize,
) -> Result<Envelope<'a>, Malformed> {
    if bytes.len() < 6 {
        return Err(Malformed::Length);
    }
    if &bytes[..4] != magic {
        return Err(Malformed::Magic);
    }
    let body_len = usize::from(u16::from_le_bytes([bytes[4], bytes[5]]));
    let len = body_len + ENVELOPE_OVERHEAD;
    if body_len == 0 || body_len > max_body || bytes.len() < len {
        return Err(Malformed::Length);
    }
    let body = &bytes[6..6 + body_len];
    let mut key_id = [0u8; 8];
    key_id.copy_from_slice(&bytes[6 + body_len..14 + body_len]);
    let mut signature = [0u8; 64];
    signature.copy_from_slice(&bytes[14 + body_len..len]);
    Ok(Envelope {
        body,
        key_id,
        signature,
        len,
    })
}

/// Reads an envelope that must fill `bytes` exactly.
pub fn open_exact<'a>(
    magic: &[u8; 4],
    bytes: &'a [u8],
    max_body: usize,
) -> Result<Envelope<'a>, Malformed> {
    let envelope = open(magic, bytes, max_body)?;
    if envelope.len != bytes.len() {
        return Err(Malformed::Trailing);
    }
    Ok(envelope)
}

fn message(domain: &[u8], body: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(domain.len() + body.len());
    message.extend_from_slice(domain);
    message.extend_from_slice(body);
    message
}

impl Envelope<'_> {
    /// Checks the signature against `keys`. The key is chosen by its identifier:
    /// no key at all → [`UpdateError::NoTrustedKeys`], identifier unknown →
    /// [`UpdateError::UnknownKey`], signature wrong → [`UpdateError::BadSignature`].
    pub fn verify(&self, domain: &[u8], keys: &[PublicKey]) -> Result<(), UpdateError> {
        if keys.is_empty() {
            return Err(UpdateError::NoTrustedKeys);
        }
        let key = keys
            .iter()
            .find(|k| key_id(k) == self.key_id)
            .ok_or(UpdateError::UnknownKey)?;
        let key = VerifyingKey::from_bytes(key).map_err(|_| UpdateError::UnknownKey)?;
        let signature = Signature::from_bytes(&self.signature);
        key.verify_strict(&message(domain, self.body), &signature)
            .map_err(|_| UpdateError::BadSignature)
    }
}

/// Signs `body` and wraps it. `None` when the body does not fit the 16-bit length or is empty.
pub fn seal(magic: &[u8; 4], domain: &[u8], body: &[u8], secret: &[u8; 32]) -> Option<Vec<u8>> {
    let body_len = u16::try_from(body.len()).ok().filter(|len| *len > 0)?;
    let key = SigningKey::from_bytes(secret);
    let signature = key.sign(&message(domain, body));
    let mut out = Vec::with_capacity(body.len() + ENVELOPE_OVERHEAD);
    out.extend_from_slice(magic);
    out.extend_from_slice(&body_len.to_le_bytes());
    out.extend_from_slice(body);
    out.extend_from_slice(&key_id(&key.verifying_key().to_bytes()));
    out.extend_from_slice(&signature.to_bytes());
    Some(out)
}

/// The public key that belongs to a secret key.
pub fn public_of(secret: &[u8; 32]) -> PublicKey {
    SigningKey::from_bytes(secret).verifying_key().to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: [u8; 32] = [7; 32];

    #[test]
    fn seal_open_verify() {
        let sealed = seal(&MANIFEST_MAGIC, MANIFEST_DOMAIN, b"hello", &SECRET).unwrap();
        assert_eq!(sealed.len(), 5 + ENVELOPE_OVERHEAD);
        let env = open_exact(&MANIFEST_MAGIC, &sealed, 100).unwrap();
        assert_eq!(env.body, b"hello");
        env.verify(MANIFEST_DOMAIN, &[public_of(&SECRET)]).unwrap();
    }

    #[test]
    fn verification_failures_are_distinguished() {
        let sealed = seal(&MANIFEST_MAGIC, MANIFEST_DOMAIN, b"hello", &SECRET).unwrap();
        let env = open(&MANIFEST_MAGIC, &sealed, 100).unwrap();
        let ours = public_of(&SECRET);
        let other = public_of(&[8; 32]);
        assert!(matches!(
            env.verify(MANIFEST_DOMAIN, &[]),
            Err(UpdateError::NoTrustedKeys)
        ));
        assert!(matches!(
            env.verify(MANIFEST_DOMAIN, &[other]),
            Err(UpdateError::UnknownKey)
        ));
        assert!(matches!(
            env.verify(KEYS_DOMAIN, &[ours]),
            Err(UpdateError::BadSignature)
        ));
        // A key id that claims to be ours over a foreign signature.
        let mut forged = seal(&MANIFEST_MAGIC, MANIFEST_DOMAIN, b"hello", &[8; 32]).unwrap();
        let id_at = 6 + 5;
        forged[id_at..id_at + 8].copy_from_slice(&key_id(&ours));
        let env = open(&MANIFEST_MAGIC, &forged, 100).unwrap();
        assert!(matches!(
            env.verify(MANIFEST_DOMAIN, &[ours, other]),
            Err(UpdateError::BadSignature)
        ));
    }

    #[test]
    fn every_flipped_bit_breaks_it() {
        let sealed = seal(&MANIFEST_MAGIC, MANIFEST_DOMAIN, b"body", &SECRET).unwrap();
        let keys = [public_of(&SECRET)];
        for byte in 0..sealed.len() {
            let mut bad = sealed.clone();
            bad[byte] ^= 0x01;
            let ok = open_exact(&MANIFEST_MAGIC, &bad, 100)
                .map_err(UpdateError::from)
                .and_then(|env| env.verify(MANIFEST_DOMAIN, &keys));
            assert!(ok.is_err(), "flip at byte {byte} was accepted");
        }
    }

    #[test]
    fn shape_errors() {
        let sealed = seal(&MANIFEST_MAGIC, MANIFEST_DOMAIN, b"hello", &SECRET).unwrap();
        assert_eq!(open(&KEYS_MAGIC, &sealed, 100), Err(Malformed::Magic));
        assert_eq!(open(&MANIFEST_MAGIC, &sealed, 4), Err(Malformed::Length));
        assert_eq!(
            open(&MANIFEST_MAGIC, &sealed[..sealed.len() - 1], 100),
            Err(Malformed::Length)
        );
        assert_eq!(open(&MANIFEST_MAGIC, b"UUM", 100), Err(Malformed::Length));
        let mut longer = sealed.clone();
        longer.push(0);
        assert_eq!(
            open_exact(&MANIFEST_MAGIC, &longer, 100),
            Err(Malformed::Trailing)
        );
        assert_eq!(
            open(&MANIFEST_MAGIC, &longer, 100).unwrap().len,
            sealed.len()
        );
        assert!(seal(&MANIFEST_MAGIC, MANIFEST_DOMAIN, b"", &SECRET).is_none());
        assert!(seal(&MANIFEST_MAGIC, MANIFEST_DOMAIN, &vec![0; 70_000], &SECRET).is_none());
        let empty_body = [b'U', b'U', b'M', b'1', 0, 0];
        assert_eq!(
            open(&MANIFEST_MAGIC, &empty_body, 100),
            Err(Malformed::Length)
        );
    }
}
