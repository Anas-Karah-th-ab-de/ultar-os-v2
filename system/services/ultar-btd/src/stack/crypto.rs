//! The LE Secure Connections functions of the Core specification (Vol 3, Part H, 2.2):
//! `f4`, `f5`, `f6`, `g2` – built on reviewed primitives only (AES-128 from `aes`,
//! AES-CMAC from `cmac`, P-256 ECDH from `p256`). Nothing here implements a primitive.
//!
//! All values are **big-endian** (most significant octet first, as the specification
//! writes them); the SMP wire format is little-endian – [`swap`] converts at the edge.

use aes::Aes128;
use cmac::{Cmac, KeyInit, Mac};
use p256::elliptic_curve::sec1::{FromSec1Point, Sec1Point, ToSec1Point};
use p256::{AffinePoint, PublicKey, SecretKey};
use zeroize::Zeroizing;

/// A failure of the cryptographic layer (never detailed further, fail closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoError {
    /// The system's random source failed.
    Random,
    /// A key is not valid (a public key not on the curve, a zero private key).
    BadKey,
}

/// Reverses a byte array (wire little-endian ⇄ specification big-endian).
pub fn swap<const N: usize>(v: &[u8; N]) -> [u8; N] {
    let mut out = *v;
    out.reverse();
    out
}

/// `N` random octets from the operating system.
pub fn random<const N: usize>() -> Result<[u8; N], CryptoError> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|_| CryptoError::Random)?;
    Ok(out)
}

fn cmac(key: &[u8; 16], parts: &[&[u8]]) -> [u8; 16] {
    // A 16-octet key always fits AES-128: `new_from_slice` cannot fail here; fail
    // closed (all zero, rejected by every comparison with a peer value) if it did.
    let Ok(mut mac) = <Cmac<Aes128> as KeyInit>::new_from_slice(key) else {
        return [0u8; 16];
    };
    for p in parts {
        mac.update(p);
    }
    mac.finalize().into_bytes().into()
}

/// `f4(U, V, X, Z) = AES-CMAC_X(U || V || Z)` – the confirm value.
pub fn f4(u: &[u8; 32], v: &[u8; 32], x: &[u8; 16], z: u8) -> [u8; 16] {
    cmac(x, &[u, v, &[z]])
}

/// `f5(W, N1, N2, A1, A2)` – (MacKey, LTK) from the DHKey.
pub fn f5(
    w: &[u8; 32],
    n1: &[u8; 16],
    n2: &[u8; 16],
    a1: &[u8; 7],
    a2: &[u8; 7],
) -> (Zeroizing<[u8; 16]>, Zeroizing<[u8; 16]>) {
    const SALT: [u8; 16] = [
        0x6c, 0x88, 0x83, 0x91, 0xaa, 0xf5, 0xa5, 0x38, 0x60, 0x37, 0x0b, 0xdb, 0x5a, 0x60, 0x83,
        0xbe,
    ];
    const KEY_ID: [u8; 4] = *b"btle";
    const LENGTH: [u8; 2] = [0x01, 0x00];
    let t = Zeroizing::new(cmac(&SALT, &[w]));
    let mac_key = cmac(&t, &[&[0], &KEY_ID, n1, n2, a1, a2, &LENGTH]);
    let ltk = cmac(&t, &[&[1], &KEY_ID, n1, n2, a1, a2, &LENGTH]);
    (Zeroizing::new(mac_key), Zeroizing::new(ltk))
}

/// `f6(W, N1, N2, R, IOcap, A1, A2)` – the DHKey check value.
pub fn f6(
    w: &[u8; 16],
    n1: &[u8; 16],
    n2: &[u8; 16],
    r: &[u8; 16],
    io_cap: &[u8; 3],
    a1: &[u8; 7],
    a2: &[u8; 7],
) -> [u8; 16] {
    cmac(w, &[n1, n2, r, io_cap, a1, a2])
}

/// `g2(U, V, X, Y)` – the six-digit number shown for numeric comparison.
pub fn g2(u: &[u8; 32], v: &[u8; 32], x: &[u8; 16], y: &[u8; 16]) -> u32 {
    let m = cmac(x, &[u, v, y]);
    u32::from_be_bytes([m[12], m[13], m[14], m[15]]) % 1_000_000
}

/// `ah(k, r)` – the hash of a resolvable private address (24 bits, big-endian).
pub fn ah(k: &[u8; 16], r: &[u8; 3]) -> [u8; 3] {
    use aes::cipher::BlockCipherEncrypt;
    let Ok(cipher) = <Aes128 as KeyInit>::new_from_slice(k) else {
        return [0; 3];
    };
    let mut block = [0u8; 16];
    block[13..].copy_from_slice(r);
    let mut b = aes::Block::from(block);
    cipher.encrypt_block(&mut b);
    [b[13], b[14], b[15]]
}

/// Whether a resolvable private address (wire order) belongs to the IRK (big-endian).
pub fn resolves(irk: &[u8; 16], addr: &[u8; 6]) -> bool {
    use subtle::ConstantTimeEq;
    if addr[5] >> 6 != 0b01 {
        return false;
    }
    let hash = ah(irk, &[addr[5], addr[4], addr[3]]);
    bool::from(hash.ct_eq(&[addr[2], addr[1], addr[0]]))
}

/// A P-256 key pair for one pairing (ephemeral, never stored).
pub struct KeyPair {
    secret: SecretKey,
    /// Public key X, big-endian.
    pub x: [u8; 32],
    /// Public key Y, big-endian.
    pub y: [u8; 32],
}

impl KeyPair {
    /// A fresh key pair from the system's random source.
    pub fn generate() -> Result<Self, CryptoError> {
        for _ in 0..8 {
            let d = Zeroizing::new(random::<32>()?);
            if let Ok(pair) = Self::from_private(&d) {
                return Ok(pair);
            }
        }
        Err(CryptoError::Random)
    }

    /// The key pair of a known private key (big-endian) – the specification's samples.
    pub fn from_private(d: &[u8; 32]) -> Result<Self, CryptoError> {
        let secret = SecretKey::from_slice(d).map_err(|_| CryptoError::BadKey)?;
        let point = secret.public_key().as_affine().to_sec1_point(false);
        let (Some(x), Some(y)) = (point.x(), point.y()) else {
            return Err(CryptoError::BadKey);
        };
        let (mut px, mut py) = ([0u8; 32], [0u8; 32]);
        px.copy_from_slice(x);
        py.copy_from_slice(y);
        Ok(Self {
            secret,
            x: px,
            y: py,
        })
    }

    /// The DHKey with a peer's public key (big-endian X, Y). The point is checked to lie
    /// on the curve (invalid-curve attacks); the identity is refused.
    pub fn dh_key(&self, x: &[u8; 32], y: &[u8; 32]) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
        let point = Sec1Point::<p256::NistP256>::from_affine_coordinates(x.into(), y.into(), false);
        let affine: Option<AffinePoint> = AffinePoint::from_sec1_point(&point).into();
        let public = affine
            .and_then(|a| PublicKey::from_affine(a).ok())
            .ok_or(CryptoError::BadKey)?;
        let shared =
            p256::ecdh::diffie_hellman(self.secret.to_nonzero_scalar(), public.as_affine());
        let mut out = Zeroizing::new([0u8; 32]);
        out.copy_from_slice(shared.raw_secret_bytes());
        Ok(out)
    }
}

#[cfg(test)]
#[path = "crypto_tests.rs"]
mod tests;
