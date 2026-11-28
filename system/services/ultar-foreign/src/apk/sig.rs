//! APK Signature Scheme v2 and v3: the signing block before the central directory is
//! found, the signature over the signed data is checked with the signer's public key,
//! and the content digest (1 MiB chunks of the entries, the directory and the end
//! record) is computed and compared. Only then is the signer's certificate trusted to
//! name the app's publisher – and to be the same at every update.
//!
//! JAR signing (v1) alone is not accepted: it does not protect the whole file.

use sha2::{Digest, Sha256, Sha512};
use std::io::{Read, Seek};

use super::zip::{Archive, EOCD_SIZE};

/// Largest signing block read.
const MAX_BLOCK: u64 = 16 << 20;
pub(super) const MAGIC: &[u8; 16] = b"APK Sig Block 42";
pub(super) const V2: u32 = 0x7109_871a;
const V3: u32 = 0xf053_68c0;
const CHUNK: u64 = 1 << 20;

/// What the signature check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signed {
    /// Verified: scheme (2 or 3) and the SHA-256 fingerprints of the signers' first
    /// certificates, with their common name.
    Valid {
        /// 2 or 3.
        scheme: u8,
        /// `(fingerprint hex, common name)` of each signer.
        signers: Vec<(String, String)>,
    },
    /// Only JAR signing (v1): not accepted.
    OldOnly,
    /// No signature at all.
    Unsigned,
    /// A signature that does not verify, or a damaged block.
    Invalid,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Algo {
    RsaPssSha256,
    RsaPssSha512,
    RsaPkcs1Sha256,
    RsaPkcs1Sha512,
    EcdsaSha256,
    EcdsaSha512,
}

impl Algo {
    fn of(id: u32) -> Option<Self> {
        Some(match id {
            0x0101 => Self::RsaPssSha256,
            0x0102 => Self::RsaPssSha512,
            0x0103 => Self::RsaPkcs1Sha256,
            0x0104 => Self::RsaPkcs1Sha512,
            0x0201 => Self::EcdsaSha256,
            0x0202 => Self::EcdsaSha512,
            _ => return None,
        })
    }

    fn sha512(self) -> bool {
        matches!(
            self,
            Self::RsaPssSha512 | Self::RsaPkcs1Sha512 | Self::EcdsaSha512
        )
    }

    /// Preference: SHA-512 before SHA-256, RSA-PSS/ECDSA before PKCS#1.
    fn rank(self) -> u8 {
        match self {
            Self::EcdsaSha512 | Self::RsaPssSha512 => 4,
            Self::RsaPkcs1Sha512 => 3,
            Self::EcdsaSha256 | Self::RsaPssSha256 => 2,
            Self::RsaPkcs1Sha256 => 1,
        }
    }
}

/// A length-prefixed reader over a byte slice.
struct Cur<'a> {
    b: &'a [u8],
}

impl<'a> Cur<'a> {
    fn u32(&mut self) -> Option<u32> {
        let (head, rest) = self.b.split_at_checked(4)?;
        self.b = rest;
        Some(u32::from_le_bytes([head[0], head[1], head[2], head[3]]))
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.b.split_at_checked(n)?;
        self.b = rest;
        Some(head)
    }

    /// A `u32`-length-prefixed part.
    fn part(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }

    fn done(&self) -> bool {
        self.b.is_empty()
    }
}

/// The ID-value pairs of the signing block and the block's start offset.
/// The ID-value pairs of a signing block.
type Pairs = Vec<(u32, Vec<u8>)>;

pub(super) fn signing_block<R: Read + Seek>(a: &mut Archive<R>) -> Option<(u64, Pairs)> {
    if a.central < 32 {
        return None;
    }
    let tail = a.raw(a.central - 24, 24).ok()?;
    if &tail[8..] != MAGIC {
        return None;
    }
    let size = u64::from_le_bytes(tail[..8].try_into().ok()?);
    if !(24..=MAX_BLOCK).contains(&size) || size + 8 > a.central {
        return None;
    }
    let start = a.central - size - 8;
    let block = a.raw(start, (size + 8) as usize).ok()?;
    if u64::from_le_bytes(block[..8].try_into().ok()?) != size {
        return None;
    }
    let mut pairs = Vec::new();
    let mut rest = &block[8..block.len() - 24];
    while !rest.is_empty() {
        let (len, after) = rest.split_at_checked(8)?;
        let len = u64::from_le_bytes(len.try_into().ok()?) as usize;
        let (pair, after) = after.split_at_checked(len)?;
        let (id, value) = pair.split_at_checked(4)?;
        pairs.push((u32::from_le_bytes(id.try_into().ok()?), value.to_vec()));
        rest = after;
    }
    Some((start, pairs))
}

/// The content digest of v2/v3 over the three sections in 1 MiB chunks (the end record
/// with its comment, its directory offset taken as the signing block's).
pub(super) fn content_digest<R: Read + Seek>(
    a: &mut Archive<R>,
    block_start: u64,
    sha512: bool,
) -> Option<Vec<u8>> {
    let end = a
        .len
        .checked_sub(a.eocd)
        .filter(|n| (EOCD_SIZE..=EOCD_SIZE + 0xffff).contains(n))?;
    let mut eocd = a.raw(a.eocd, end as usize).ok()?;
    eocd[16..20].copy_from_slice(&u32::try_from(block_start).ok()?.to_le_bytes());
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let digest = |data: &[u8]| -> Vec<u8> {
        let mut prefix = vec![0xa5u8];
        prefix.extend_from_slice(&(data.len() as u32).to_le_bytes());
        if sha512 {
            Sha512::new()
                .chain_update(&prefix)
                .chain_update(data)
                .finalize()
                .to_vec()
        } else {
            Sha256::new()
                .chain_update(&prefix)
                .chain_update(data)
                .finalize()
                .to_vec()
        }
    };
    for (from, to) in [(0, block_start), (a.central, a.eocd)] {
        let mut at = from;
        while at < to {
            let n = (to - at).min(CHUNK);
            chunks.push(digest(&a.raw(at, n as usize).ok()?));
            at += n;
        }
    }
    chunks.push(digest(&eocd));
    let mut top = vec![0x5au8];
    top.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
    for c in &chunks {
        top.extend_from_slice(c);
    }
    Some(if sha512 {
        Sha512::digest(&top).to_vec()
    } else {
        Sha256::digest(&top).to_vec()
    })
}

fn verify(algo: Algo, spki: &[u8], message: &[u8], signature: &[u8]) -> bool {
    use rsa::pkcs8::DecodePublicKey;
    use rsa::signature::Verifier;
    match algo {
        Algo::RsaPkcs1Sha256 | Algo::RsaPkcs1Sha512 | Algo::RsaPssSha256 | Algo::RsaPssSha512 => {
            let Ok(key) = rsa::RsaPublicKey::from_public_key_der(spki) else {
                return false;
            };
            match algo {
                Algo::RsaPkcs1Sha256 => {
                    rsa::pkcs1v15::Signature::try_from(signature).is_ok_and(|s| {
                        rsa::pkcs1v15::VerifyingKey::<Sha256>::new(key)
                            .verify(message, &s)
                            .is_ok()
                    })
                }
                Algo::RsaPkcs1Sha512 => {
                    rsa::pkcs1v15::Signature::try_from(signature).is_ok_and(|s| {
                        rsa::pkcs1v15::VerifyingKey::<Sha512>::new(key)
                            .verify(message, &s)
                            .is_ok()
                    })
                }
                Algo::RsaPssSha256 => rsa::pss::Signature::try_from(signature).is_ok_and(|s| {
                    rsa::pss::VerifyingKey::<Sha256>::new(key)
                        .verify(message, &s)
                        .is_ok()
                }),
                _ => rsa::pss::Signature::try_from(signature).is_ok_and(|s| {
                    rsa::pss::VerifyingKey::<Sha512>::new(key)
                        .verify(message, &s)
                        .is_ok()
                }),
            }
        }
        Algo::EcdsaSha256 | Algo::EcdsaSha512 => verify_ec(algo, spki, message, signature),
    }
}

fn verify_ec(algo: Algo, spki: &[u8], message: &[u8], signature: &[u8]) -> bool {
    use ecdsa::signature::hazmat::PrehashVerifier;
    use p256::pkcs8::DecodePublicKey;
    let prehash: Vec<u8> = if algo == Algo::EcdsaSha512 {
        Sha512::digest(message).to_vec()
    } else {
        Sha256::digest(message).to_vec()
    };
    if let Ok(key) = p256::ecdsa::VerifyingKey::from_public_key_der(spki) {
        return p256::ecdsa::Signature::from_der(signature)
            .is_ok_and(|s| key.verify_prehash(&prehash, &s).is_ok());
    }
    if let Ok(key) = p384::ecdsa::VerifyingKey::from_public_key_der(spki) {
        return p384::ecdsa::Signature::from_der(signature)
            .is_ok_and(|s| key.verify_prehash(&prehash, &s).is_ok());
    }
    false
}

/// One DER element at the start of `b`: (tag, contents, rest).
fn der(b: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let tag = *b.first()?;
    let first = *b.get(1)? as usize;
    let (len, head) = if first < 0x80 {
        (first, 2)
    } else {
        let n = first & 0x7f;
        if n == 0 || n > 4 {
            return None;
        }
        let mut len = 0usize;
        for i in 0..n {
            len = (len << 8) | *b.get(2 + i)? as usize;
        }
        (len, 2 + n)
    };
    let body = b.get(head..head + len)?;
    Some((tag, body, &b[head + len..]))
}

/// The subject's common name and the SubjectPublicKeyInfo (whole element) of a
/// certificate.
fn certificate_parts(cert: &[u8]) -> Option<(String, Vec<u8>)> {
    let (_, outer, _) = der(cert)?;
    let (_, tbs, _) = der(outer)?;
    let mut rest = tbs;
    if rest.first() == Some(&0xa0) {
        rest = der(rest)?.2; // version
    }
    for _ in 0..3 {
        rest = der(rest)?.2; // serial, signature, issuer
    }
    rest = der(rest)?.2; // validity
    let (_, subject, after) = der(rest)?;
    let spki_len = after.len() - der(after)?.2.len();
    let spki = after[..spki_len].to_vec();
    let mut cn = String::new();
    let mut sets = subject;
    while let Some((_, set, next)) = der(sets) {
        if let Some((_, pair, _)) = der(set) {
            if let Some((_, oid, value)) = der(pair) {
                if oid == [0x55, 0x04, 0x03] {
                    if let Some((_, v, _)) = der(value) {
                        cn = String::from_utf8_lossy(v).chars().take(64).collect();
                    }
                }
            }
        }
        sets = next;
    }
    Some((cn, spki))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Checks one signer of a v2 (`v3 = false`) or v3 block. Returns its first certificate.
fn signer<R: Read + Seek>(
    a: &mut Archive<R>,
    block_start: u64,
    data: &[u8],
    v3: bool,
) -> Option<Vec<u8>> {
    let mut s = Cur { b: data };
    let signed = s.part()?;
    if v3 {
        s.u32()?; // min sdk
        s.u32()?; // max sdk
    }
    let mut sigs = Cur { b: s.part()? };
    let public_key = s.part()?;
    let mut best: Option<(Algo, &[u8])> = None;
    let mut sig_algos = Vec::new();
    while !sigs.done() {
        let mut one = Cur { b: sigs.part()? };
        let id = one.u32()?;
        let sig = one.part()?;
        sig_algos.push(id);
        if let Some(algo) = Algo::of(id) {
            if best.is_none_or(|(b, _)| algo.rank() > b.rank()) {
                best = Some((algo, sig));
            }
        }
    }
    let (algo, signature) = best?;
    if !verify(algo, public_key, signed, signature) {
        return None;
    }
    let mut sd = Cur { b: signed };
    let mut digests = Cur { b: sd.part()? };
    let mut certs = Cur { b: sd.part()? };
    let mut digest_algos = Vec::new();
    let mut expected = None;
    while !digests.done() {
        let mut one = Cur { b: digests.part()? };
        let id = one.u32()?;
        let d = one.part()?;
        digest_algos.push(id);
        if Algo::of(id) == Some(algo) {
            expected = Some(d.to_vec());
        }
    }
    if digest_algos != sig_algos {
        return None;
    }
    let cert = certs.part()?.to_vec();
    let (_, spki) = certificate_parts(&cert)?;
    if spki != public_key {
        return None;
    }
    let actual = content_digest(a, block_start, algo.sha512())?;
    (Some(actual) == expected).then_some(cert)
}

/// Checks the APK's signature (v3 first, then v2).
pub fn check<R: Read + Seek>(a: &mut Archive<R>) -> Signed {
    let Some((start, pairs)) = signing_block(a) else {
        let v1 = a.entries.iter().any(|e| {
            e.name.starts_with("META-INF/")
                && (e.name.ends_with(".RSA") || e.name.ends_with(".EC") || e.name.ends_with(".DSA"))
        });
        return if v1 {
            Signed::OldOnly
        } else {
            Signed::Unsigned
        };
    };
    for (id, scheme) in [(V3, 3u8), (V2, 2u8)] {
        let Some((_, value)) = pairs.iter().find(|(i, _)| *i == id) else {
            continue;
        };
        let mut list = Cur { b: value };
        let Some(mut signers) = list.part().map(|b| Cur { b }) else {
            return Signed::Invalid;
        };
        let mut found = Vec::new();
        while !signers.done() {
            let Some(data) = signers.part() else {
                return Signed::Invalid;
            };
            match signer(a, start, data, scheme == 3) {
                Some(cert) => {
                    let cn = certificate_parts(&cert)
                        .map(|(cn, _)| cn)
                        .unwrap_or_default();
                    found.push((hex(&Sha256::digest(&cert)), cn));
                }
                None => return Signed::Invalid,
            }
        }
        if found.is_empty() || found.len() > 8 {
            return Signed::Invalid;
        }
        return Signed::Valid {
            scheme,
            signers: found,
        };
    }
    Signed::Unsigned
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apk::zip::tests::build;
    use std::io::Cursor;

    #[test]
    fn an_unsigned_zip_and_a_v1_only_zip_are_told_apart() {
        let plain = build(&[("a", b"x")], false);
        let mut a = Archive::open(Cursor::new(plain)).unwrap();
        assert_eq!(check(&mut a), Signed::Unsigned);
        let v1 = build(&[("META-INF/CERT.RSA", b"x"), ("a", b"y")], false);
        let mut a = Archive::open(Cursor::new(v1)).unwrap();
        assert_eq!(check(&mut a), Signed::OldOnly);
    }

    #[test]
    fn a_planted_block_with_garbage_is_invalid_not_valid() {
        // a ZIP with a fake signing block before its directory
        let zip = build(&[("a", b"x")], false);
        let central =
            u32::from_le_bytes(zip[zip.len() - 6..zip.len() - 2].try_into().unwrap()) as usize;
        let mut pair = Vec::new();
        pair.extend_from_slice(&V2.to_le_bytes());
        pair.extend_from_slice(&[4, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
        let mut block = Vec::new();
        let size = (8 + pair.len() + 24) as u64;
        block.extend_from_slice(&size.to_le_bytes());
        block.extend_from_slice(&(pair.len() as u64).to_le_bytes());
        block.extend_from_slice(&pair);
        block.extend_from_slice(&size.to_le_bytes());
        block.extend_from_slice(MAGIC);
        let mut apk = zip[..central].to_vec();
        apk.extend_from_slice(&block);
        let new_central = apk.len() as u32;
        apk.extend_from_slice(&zip[central..]);
        let n = apk.len();
        apk[n - 6..n - 2].copy_from_slice(&new_central.to_le_bytes());
        let mut a = Archive::open(Cursor::new(apk)).unwrap();
        assert_eq!(check(&mut a), Signed::Invalid);
    }

    #[test]
    fn der_walks_stay_in_bounds() {
        assert!(der(&[0x30, 0x84, 0xff, 0xff, 0xff, 0xff]).is_none());
        assert!(der(&[0x30]).is_none());
        assert!(certificate_parts(&[0x30, 0x03, 0x30, 0x01, 0x00]).is_none());
    }
}
