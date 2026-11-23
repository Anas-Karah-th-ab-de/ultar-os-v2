//! Device identity: key-derived device ids, signed transcripts and their verification.
//!
//! A device is identified by an Ed25519 public key. Its id is the lower-case base32 form of the
//! first 160 bits of `SHA-256("ultar-device-v1" ‖ public key)`. Every signature a device makes is
//! over a domain-separated, length-framed transcript ([`Transcript`]); see
//! `docs/DEVICE-IDENTITY.md` §3–§4. Only standard primitives are used (`ed25519-dalek`, `sha2`).

use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use std::fmt;

/// Length of an Ed25519 public key in bytes.
pub const PUBLIC_KEY_BYTES: usize = 32;
/// Length of an Ed25519 signature in bytes.
pub const SIGNATURE_BYTES: usize = 64;
/// Length of a challenge in bytes.
pub const CHALLENGE_BYTES: usize = 32;
/// Length of a key-derived device id in characters.
pub const DEVICE_ID_LEN: usize = 32;
/// Prefix of the ids of devices registered before device keys existed.
pub const LEGACY_PREFIX: &str = "legacy:";
/// Characters of a device id that may appear in logs and notices.
pub const LOG_PREFIX_LEN: usize = 6;

const ID_LABEL: &[u8] = b"ultar-device-v1";
const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// What a signature is for. Each purpose has its own transcript label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Purpose {
    /// Registration of a new device key.
    Register,
    /// Issue of a device token.
    Session,
    /// Authentication of the signalling socket.
    Signal,
    /// Upgrade of a registration from before device keys.
    Upgrade,
    /// Replacement, signed by the old device.
    ReplaceOld,
    /// Replacement, signed by the new device.
    ReplaceNew,
    /// Replacement of a lost device, signed by the new device.
    Recover,
}

impl Purpose {
    /// The domain-separation label that starts the transcript.
    pub fn label(self) -> &'static str {
        match self {
            Purpose::Register => "ultar-device-register-v1",
            Purpose::Session => "ultar-device-session-v1",
            Purpose::Signal => "ultar-device-signal-v1",
            Purpose::Upgrade => "ultar-device-upgrade-v1",
            Purpose::ReplaceOld => "ultar-device-replace-old-v1",
            Purpose::ReplaceNew => "ultar-device-replace-new-v1",
            Purpose::Recover => "ultar-device-recover-v1",
        }
    }
}

/// Why a proof of possession was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopError {
    /// The public key is not 32 bytes of base64url or not a point on the curve.
    MalformedKey,
    /// The public key is a small-order point.
    WeakKey,
    /// The signature is not 64 bytes of base64url.
    MalformedSignature,
    /// The signature does not verify for this key and transcript.
    BadSignature,
}

impl fmt::Display for PopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PopError::MalformedKey => "malformed public key",
            PopError::WeakKey => "weak public key",
            PopError::MalformedSignature => "malformed signature",
            PopError::BadSignature => "signature does not verify",
        })
    }
}

impl std::error::Error for PopError {}

/// RFC 4648 base32 with the lower-case alphabet and without padding.
pub fn base32_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let (mut acc, mut bits) = (0u32, 0u32);
    for byte in bytes {
        acc = (acc << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(BASE32[((acc >> bits) & 31) as usize]));
        }
    }
    if bits > 0 {
        out.push(char::from(BASE32[((acc << (5 - bits)) & 31) as usize]));
    }
    out
}

/// The device id of a public key.
pub fn device_id(public_key: &[u8; PUBLIC_KEY_BYTES]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ID_LABEL);
    hasher.update(public_key);
    base32_lower(&hasher.finalize()[..20])
}

/// True when `text` has the shape of a key-derived device id.
pub fn is_device_id(text: &str) -> bool {
    text.len() == DEVICE_ID_LEN && text.bytes().all(|b| BASE32.contains(&b))
}

/// The first `len` characters of an id, for logs and notices (never the whole id).
pub fn id_prefix(id: &str, len: usize) -> String {
    id.strip_prefix(LEGACY_PREFIX)
        .unwrap_or(id)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(len)
        .collect()
}

/// Decodes a base64url value of exactly `N` bytes.
pub fn decode_fixed<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() > N * 2 {
        return None;
    }
    <[u8; N]>::try_from(B64.decode(text).ok()?.as_slice()).ok()
}

/// Encodes bytes as base64url without padding.
pub fn encode(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

/// Parses and validates a public key (base64url). Weak keys are refused.
pub fn parse_public_key(text: &str) -> Result<[u8; PUBLIC_KEY_BYTES], PopError> {
    let bytes = decode_fixed::<PUBLIC_KEY_BYTES>(text).ok_or(PopError::MalformedKey)?;
    let key = VerifyingKey::from_bytes(&bytes).map_err(|_| PopError::MalformedKey)?;
    if key.is_weak() {
        return Err(PopError::WeakKey);
    }
    Ok(bytes)
}

/// Parses a signature (base64url).
pub fn parse_signature(text: &str) -> Result<[u8; SIGNATURE_BYTES], PopError> {
    decode_fixed::<SIGNATURE_BYTES>(text).ok_or(PopError::MalformedSignature)
}

/// The byte string a device signs.
#[derive(Debug, Clone, Copy)]
pub struct Transcript<'a> {
    /// What the signature is for.
    pub purpose: Purpose,
    /// Name of the server the signature is meant for.
    pub server: &'a str,
    /// Account the signature is meant for.
    pub account_id: i64,
    /// The server's challenge.
    pub challenge: &'a [u8; CHALLENGE_BYTES],
    /// Public key of the signer.
    pub subject_key: &'a [u8; PUBLIC_KEY_BYTES],
    /// Purpose-specific data (empty, a device id or the other party's key).
    pub extra: &'a [u8],
}

fn framed(out: &mut Vec<u8>, field: &[u8]) {
    let len = u16::try_from(field.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(field);
}

impl Transcript<'_> {
    /// `label ‖ 0 ‖ len16 ‖ server ‖ account(be64) ‖ challenge ‖ subject key ‖ len16 ‖ extra`.
    pub fn bytes(&self) -> Vec<u8> {
        let label = self.purpose.label().as_bytes();
        let mut out = Vec::with_capacity(label.len() + self.server.len() + self.extra.len() + 80);
        out.extend_from_slice(label);
        out.push(0);
        framed(&mut out, self.server.as_bytes());
        out.extend_from_slice(&self.account_id.to_be_bytes());
        out.extend_from_slice(self.challenge);
        out.extend_from_slice(self.subject_key);
        framed(&mut out, self.extra);
        out
    }

    /// Verifies `signature` over this transcript under the subject key (strict Ed25519: small-order
    /// keys and non-canonical signatures are refused).
    pub fn verify(&self, signature: &[u8; SIGNATURE_BYTES]) -> Result<(), PopError> {
        // Fields longer than the 16-bit frame cannot be represented unambiguously.
        if self.server.len() > usize::from(u16::MAX) || self.extra.len() > usize::from(u16::MAX) {
            return Err(PopError::BadSignature);
        }
        let key = VerifyingKey::from_bytes(self.subject_key).map_err(|_| PopError::MalformedKey)?;
        if key.is_weak() {
            return Err(PopError::WeakKey);
        }
        key.verify_strict(&self.bytes(), &Signature::from_bytes(signature))
            .map_err(|_| PopError::BadSignature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn base32_matches_rfc_4648_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "my"),
            ("fo", "mzxq"),
            ("foo", "mzxw6"),
            ("foob", "mzxw6yq"),
            ("fooba", "mzxw6ytb"),
            ("foobar", "mzxw6ytboi"),
        ] {
            assert_eq!(base32_lower(input.as_bytes()), expected);
        }
    }

    #[test]
    fn device_id_is_fixed_for_a_key_and_differs_between_keys() {
        let a = device_id(&key(1).verifying_key().to_bytes());
        assert_eq!(a, device_id(&key(1).verifying_key().to_bytes()));
        assert!(is_device_id(&a), "{a}");
        assert_ne!(a, device_id(&key(2).verifying_key().to_bytes()));
        // Fixed vector shared with the device client (ultar-cloud/src/identity.rs).
        assert_eq!(device_id(&[0u8; 32]), "44ifisy5d2lrobapoboy7iir4a2ksvm7");
        assert!(!is_device_id("legacy:abc"));
        assert!(!is_device_id(&a.to_uppercase()));
        assert!(!is_device_id(&a[1..]));
    }

    #[test]
    fn prefixes_never_carry_the_whole_id_or_odd_characters() {
        assert_eq!(id_prefix("abcdefghijkl", LOG_PREFIX_LEN), "abcdef");
        assert_eq!(id_prefix("legacy:7af8c5b4c52e91a3", 6), "7af8c5");
        assert_eq!(id_prefix("legacy:a\nb\"c", 6), "abc");
        assert_eq!(id_prefix("", 6), "");
    }

    fn transcript<'a>(
        purpose: Purpose,
        challenge: &'a [u8; 32],
        pk: &'a [u8; 32],
        extra: &'a [u8],
    ) -> Transcript<'a> {
        Transcript {
            purpose,
            server: "ultar.example",
            account_id: 7,
            challenge,
            subject_key: pk,
            extra,
        }
    }

    #[test]
    fn transcript_layout_is_as_documented() {
        let (challenge, pk) = ([0xAA; 32], [0xBB; 32]);
        let bytes = transcript(Purpose::Register, &challenge, &pk, b"xy").bytes();
        let mut expected = b"ultar-device-register-v1\0".to_vec();
        expected.extend_from_slice(&[0, 13]);
        expected.extend_from_slice(b"ultar.example");
        expected.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 7]);
        expected.extend_from_slice(&[0xAA; 32]);
        expected.extend_from_slice(&[0xBB; 32]);
        expected.extend_from_slice(&[0, 2, b'x', b'y']);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn a_signature_verifies_only_for_its_exact_transcript() {
        let sk = key(3);
        let pk = sk.verifying_key().to_bytes();
        let challenge = [9u8; 32];
        let good = transcript(Purpose::Register, &challenge, &pk, b"");
        let sig = sk.sign(&good.bytes()).to_bytes();
        assert_eq!(good.verify(&sig), Ok(()));

        let other_challenge = [8u8; 32];
        let other_pk = key(4).verifying_key().to_bytes();
        let variants = [
            transcript(Purpose::Session, &challenge, &pk, b""),
            transcript(Purpose::Signal, &challenge, &pk, b""),
            transcript(Purpose::Register, &other_challenge, &pk, b""),
            transcript(Purpose::Register, &challenge, &other_pk, b""),
            transcript(Purpose::Register, &challenge, &pk, b"x"),
            Transcript {
                account_id: 8,
                ..good
            },
            Transcript {
                server: "ultar.example.evil",
                ..good
            },
        ];
        for t in variants {
            assert_eq!(t.verify(&sig), Err(PopError::BadSignature), "{t:?}");
        }
        let mut flipped = sig;
        flipped[10] ^= 1;
        assert_eq!(good.verify(&flipped), Err(PopError::BadSignature));
    }

    #[test]
    fn labels_are_distinct_and_no_label_is_a_prefix_of_another() {
        let all = [
            Purpose::Register,
            Purpose::Session,
            Purpose::Signal,
            Purpose::Upgrade,
            Purpose::ReplaceOld,
            Purpose::ReplaceNew,
            Purpose::Recover,
        ];
        for a in all {
            for b in all {
                if a != b {
                    let (x, y) = (format!("{}\0", a.label()), format!("{}\0", b.label()));
                    assert!(!x.starts_with(&y), "{x:?} {y:?}");
                }
            }
        }
    }

    #[test]
    fn framing_keeps_field_boundaries_apart() {
        let (challenge, pk) = ([1u8; 32], [2u8; 32]);
        let a = Transcript {
            server: "ab",
            ..transcript(Purpose::Upgrade, &challenge, &pk, b"c")
        };
        let b = Transcript {
            server: "a",
            ..transcript(Purpose::Upgrade, &challenge, &pk, b"bc")
        };
        assert_ne!(a.bytes(), b.bytes());
    }

    #[test]
    fn malformed_and_weak_inputs_are_refused() {
        assert_eq!(parse_public_key(""), Err(PopError::MalformedKey));
        assert_eq!(parse_public_key("AAAA"), Err(PopError::MalformedKey));
        assert_eq!(
            parse_public_key(&"A".repeat(500)),
            Err(PopError::MalformedKey)
        );
        // The identity point (y = 1) has small order.
        let mut identity = [0u8; 32];
        identity[0] = 1;
        assert_eq!(parse_public_key(&encode(&identity)), Err(PopError::WeakKey));
        let good = key(5).verifying_key().to_bytes();
        assert_eq!(parse_public_key(&encode(&good)), Ok(good));
        assert_eq!(parse_signature("x"), Err(PopError::MalformedSignature));
        assert!(parse_signature(&encode(&[0u8; 64])).is_ok());
        let challenge = [0u8; 32];
        let weak = transcript(Purpose::Register, &challenge, &identity, b"");
        assert_eq!(weak.verify(&[0u8; 64]), Err(PopError::WeakKey));
        assert!(PopError::BadSignature.to_string().contains("verify"));
    }
}
