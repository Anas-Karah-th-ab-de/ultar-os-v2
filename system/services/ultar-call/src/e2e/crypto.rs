//! End-to-end keys of one call. The server relays the handshake but can neither read nor
//! change what follows, nor stand in for either side:
//!
//! 1. caller → `hello`: call id, ephemeral X25519 key `ea`, caller's device key `da`;
//! 2. callee → `reply`: ephemeral `eb`, device key `db`, signature of the callee's device key
//!    over the transcript `T = SHA-256(label ‖ call ‖ ea ‖ eb ‖ da ‖ db)`;
//! 3. caller checks it and sends, sealed, its own signature over `T` – the callee rings only
//!    after checking that one.
//!
//! Keys: HKDF-SHA-256 over the X25519 secret with `T` as salt, one key per direction for the
//! signalling messages and one per direction for the media (ChaCha20-Poly1305, 64-bit counter
//! nonces – a key never sees a nonce twice). The device keys sign only this transcript, under
//! a label no other Ultar protocol uses. Whether `da`/`db` really belong to the number is
//! pinned on first contact and shown as a safety number ([`safety_number`]).

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use x25519_dalek::{EphemeralSecret, PublicKey};

/// Label of the transcript; the device key signs `SIGN_LABEL ‖ T` and nothing else for calls.
pub const SIGN_LABEL: &[u8] = ultar_bus::messages::account_device::CALL_SIGN_LABEL;
/// Length of a call id.
pub const CALL_ID: usize = 16;

/// Which side of the call this device is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Dialled.
    Caller,
    /// Was called.
    Callee,
}

/// Why keys could not be made or a message could not be opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CryptoError {
    /// A key or signature has the wrong form.
    Malformed,
    /// A signature does not verify.
    BadSignature,
    /// Authentication of a sealed message failed (changed, replayed or not for this call).
    Open,
}

/// The ephemeral half of one side, before the other side's key is known.
pub struct Ephemeral {
    secret: EphemeralSecret,
    /// Its public key.
    pub public: [u8; 32],
}

impl Ephemeral {
    /// A fresh key.
    pub fn new() -> Self {
        let secret = EphemeralSecret::random_from_rng(rand_core::OsRng);
        let public = PublicKey::from(&secret).to_bytes();
        Self { secret, public }
    }
}

impl Default for Ephemeral {
    fn default() -> Self {
        Self::new()
    }
}

/// The public facts of a handshake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transcript {
    /// Call id (random, chosen by the caller).
    pub call: [u8; CALL_ID],
    /// Caller's ephemeral key.
    pub ea: [u8; 32],
    /// Callee's ephemeral key.
    pub eb: [u8; 32],
    /// Caller's device key (Ed25519).
    pub da: [u8; 32],
    /// Callee's device key (Ed25519).
    pub db: [u8; 32],
}

impl Transcript {
    /// `T`.
    pub fn hash(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b"ultar-call-v1");
        for part in [&self.call[..], &self.ea, &self.eb, &self.da, &self.db] {
            h.update(part);
        }
        h.finalize().into()
    }

    /// What the device key signs: label ‖ `T`.
    pub fn to_sign(&self) -> Vec<u8> {
        let mut m = SIGN_LABEL.to_vec();
        m.extend_from_slice(&self.hash());
        m
    }
}

/// Checks the signature of device key `key` over the transcript.
pub fn verify(t: &Transcript, key: &[u8; 32], sig: &[u8]) -> Result<(), CryptoError> {
    let key = VerifyingKey::from_bytes(key).map_err(|_| CryptoError::Malformed)?;
    let sig = Signature::from_slice(sig).map_err(|_| CryptoError::Malformed)?;
    key.verify(&t.to_sign(), &sig)
        .map_err(|_| CryptoError::BadSignature)
}

/// A one-way sealing key with its counter.
pub struct Sealer {
    aead: ChaCha20Poly1305,
    next: u64,
}

/// The receiving half: key and replay window.
pub struct Opener {
    aead: ChaCha20Poly1305,
    window: crate::e2e::replay::Window,
}

fn nonce(counter: u64) -> Nonce {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&counter.to_be_bytes());
    Nonce::from(n)
}

impl Sealer {
    /// Seals `plain`: `counter (8) ‖ ciphertext`. `aad` is authenticated, not hidden.
    pub fn seal(&mut self, aad: &[u8], plain: &[u8]) -> Vec<u8> {
        let counter = self.next;
        self.next += 1;
        let payload = chacha20poly1305::aead::Payload { msg: plain, aad };
        // Encryption of a well-formed payload with a fresh nonce cannot fail.
        let sealed = self
            .aead
            .encrypt(&nonce(counter), payload)
            .unwrap_or_default();
        let mut out = counter.to_be_bytes().to_vec();
        out.extend_from_slice(&sealed);
        out
    }
}

impl Opener {
    /// Opens what [`Sealer::seal`] made; a replayed or changed message is refused.
    pub fn open(&mut self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let (head, body) = sealed.split_at_checked(8).ok_or(CryptoError::Open)?;
        let counter = u64::from_be_bytes(head.try_into().map_err(|_| CryptoError::Open)?);
        if !self.window.fresh(counter) {
            return Err(CryptoError::Open);
        }
        let payload = chacha20poly1305::aead::Payload { msg: body, aad };
        let plain = self
            .aead
            .decrypt(&nonce(counter), payload)
            .map_err(|_| CryptoError::Open)?;
        self.window.mark(counter);
        Ok(plain)
    }
}

/// The media keys of one call for this device.
pub struct MediaKeys {
    /// Media it sends.
    pub out: Sealer,
    /// Media it receives.
    pub inn: Opener,
}

/// The keys of one call for this device.
pub struct CallKeys {
    /// Signalling messages this device sends.
    pub signal_out: Sealer,
    /// Signalling messages it receives.
    pub signal_in: Opener,
    /// The media keys (taken by the media part when the call starts).
    pub media: Option<MediaKeys>,
    /// `T` (for signatures and the confirmation).
    pub transcript: [u8; 32],
}

fn derive(secret: &[u8; 32], salt: &[u8; 32], info: &[u8]) -> ChaCha20Poly1305 {
    let hk = Hkdf::<Sha256>::new(Some(salt), secret);
    let mut key = [0u8; 32];
    // 32 bytes are far below HKDF's limit of 255 · 32.
    let _ = hk.expand(info, &mut key);
    ChaCha20Poly1305::new(Key::from_slice(&key))
}

/// Finishes the key exchange: own ephemeral secret, the other side's ephemeral public key.
pub fn keys(
    own: Ephemeral,
    other: &[u8; 32],
    t: &Transcript,
    role: Role,
) -> Result<CallKeys, CryptoError> {
    let shared = own.secret.diffie_hellman(&PublicKey::from(*other));
    if !shared.was_contributory() {
        return Err(CryptoError::Malformed);
    }
    let salt = t.hash();
    let s = shared.as_bytes();
    let (out, inn) = match role {
        Role::Caller => (b"a2b", b"b2a"),
        Role::Callee => (b"b2a", b"a2b"),
    };
    let sealer = |kind: &[u8], dir: &[u8]| Sealer {
        aead: derive(s, &salt, &[kind, dir].concat()),
        next: 0,
    };
    let opener = |kind: &[u8], dir: &[u8]| Opener {
        aead: derive(s, &salt, &[kind, dir].concat()),
        window: crate::e2e::replay::Window::default(),
    };
    Ok(CallKeys {
        signal_out: sealer(b"signal ", out),
        signal_in: opener(b"signal ", inn),
        media: Some(MediaKeys {
            out: sealer(b"media ", out),
            inn: opener(b"media ", inn),
        }),
        transcript: salt,
    })
}

/// The safety number of two devices: 12 groups of 5 digits, the same on both sides and for
/// every call between them. Changes only when a device key changes.
pub fn safety_number(a: &[u8; 32], b: &[u8; 32]) -> String {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let mut h = Sha256::new();
    h.update(b"ultar-safety-number-v1");
    h.update(lo);
    h.update(hi);
    let d: [u8; 32] = h.finalize().into();
    (0..12)
        .map(|i| {
            let mut v = [0u8; 4];
            v[1..].copy_from_slice(&d[i * 2..i * 2 + 3]);
            format!("{:05}", u32::from_be_bytes(v) % 100_000)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
#[path = "crypto_tests.rs"]
mod tests;
