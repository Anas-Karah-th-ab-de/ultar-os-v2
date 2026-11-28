//! Test signing (only with the feature `test-sign` and in the unit tests, never in the
//! service): signs an APK again with APK Signature Scheme v2 and an ECDSA P-256 key made
//! from a name, and makes a damaged copy. Used to test that the same package from another
//! signer is refused as an update, and that a changed file is refused
//! (`examples/apk-variant.rs`, `tools/foreign/android-e2e.sh`).

use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use sha2::{Digest, Sha256};
use std::io::Cursor;

use super::sig::{content_digest, signing_block, MAGIC, V2};
use super::zip::Archive;

/// `ecdsa-with-SHA256`, APK signature algorithm 0x0201.
const ECDSA_SHA256_OID: [u8; 8] = [0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];
const ECDSA_SHA256: u32 = 0x0201;

fn lp(b: &[u8]) -> Vec<u8> {
    let mut v = u32::try_from(b.len())
        .unwrap_or(u32::MAX)
        .to_le_bytes()
        .to_vec();
    v.extend_from_slice(b);
    v
}

fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let n = content.len();
    if n < 0x80 {
        out.push(n as u8);
    } else if n < 0x100 {
        out.extend_from_slice(&[0x81, n as u8]);
    } else {
        out.extend_from_slice(&[0x82, (n >> 8) as u8, n as u8]);
    }
    out.extend_from_slice(content);
    out
}

fn name(cn: &str) -> Vec<u8> {
    let pair = [tlv(0x06, &[0x55, 0x04, 0x03]), tlv(0x0c, cn.as_bytes())].concat();
    tlv(0x30, &tlv(0x31, &tlv(0x30, &pair)))
}

/// A self-signed X.509 certificate for `key` with the common name `cn`.
fn certificate(key: &SigningKey, spki: &[u8], cn: &str) -> Vec<u8> {
    let algorithm = tlv(0x30, &tlv(0x06, &ECDSA_SHA256_OID));
    let validity = [tlv(0x17, b"250101000000Z"), tlv(0x17, b"491231235959Z")].concat();
    let tbs = tlv(
        0x30,
        &[
            tlv(0xa0, &tlv(0x02, &[2])),
            tlv(0x02, &[1]),
            algorithm.clone(),
            name(cn),
            tlv(0x30, &validity),
            name(cn),
            spki.to_vec(),
        ]
        .concat(),
    );
    let signature: Signature = key.sign(&tbs);
    let bits = [&[0u8][..], signature.to_der().as_bytes()].concat();
    tlv(0x30, &[tbs, algorithm, tlv(0x03, &bits)].concat())
}

/// The signer's public key as `SubjectPublicKeyInfo` (id-ecPublicKey, prime256v1).
fn spki(key: &SigningKey) -> Vec<u8> {
    let algorithm = [
        tlv(0x06, &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01]),
        tlv(0x06, &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07]),
    ]
    .concat();
    let point = key.verifying_key().to_encoded_point(false);
    let bits = [&[0u8][..], point.as_bytes()].concat();
    tlv(0x30, &[tlv(0x30, &algorithm), tlv(0x03, &bits)].concat())
}

/// The key of a test signer (made from its name: the same name, the same signer).
fn key(signer: &str) -> SigningKey {
    let mut seed = Sha256::digest(signer.as_bytes());
    // a scalar below the curve order (it practically always is; this keeps it certain)
    seed[0] &= 0x7f;
    seed[31] |= 1;
    SigningKey::from_bytes(&seed).unwrap_or_else(|_| unreachable!("a valid scalar"))
}

/// `apk` signed again (v2 only; any earlier signing block is replaced) by the test
/// signer `signer`, whose certificate's common name is `signer`.
pub fn resign(apk: &[u8], signer: &str) -> Option<Vec<u8>> {
    let mut a = Archive::open(Cursor::new(apk)).ok()?;
    let start = signing_block(&mut a).map_or(a.central, |(s, _)| s);
    let digest = content_digest(&mut a, start, false)?;
    let (start, central, eocd) = (start as usize, a.central as usize, a.eocd as usize);
    let key = key(signer);
    let spki = spki(&key);
    let cert = certificate(&key, &spki, signer);
    let algo = ECDSA_SHA256.to_le_bytes();
    let signed = [
        lp(&lp(&[&algo[..], &lp(&digest)].concat())),
        lp(&lp(&cert)),
        lp(&[]),
    ]
    .concat();
    let signature: Signature = key.sign(&signed);
    let signatures = lp(&lp(
        &[&algo[..], &lp(signature.to_der().as_bytes())].concat()
    ));
    let one = [lp(&signed), signatures, lp(&spki)].concat();
    let mut pair = V2.to_le_bytes().to_vec();
    pair.extend_from_slice(&lp(&lp(&one)));
    let size = (8 + pair.len() + 8 + 16) as u64;
    let mut out = apk[..start].to_vec();
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&(pair.len() as u64).to_le_bytes());
    out.extend_from_slice(&pair);
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(MAGIC);
    let new_central = u32::try_from(out.len()).ok()?;
    out.extend_from_slice(&apk[central..eocd]);
    let mut end = apk[eocd..].to_vec();
    end[16..20].copy_from_slice(&new_central.to_le_bytes());
    out.extend_from_slice(&end);
    Some(out)
}

/// A copy of `apk` with one byte of the stored data of `entry` changed (the ZIP stays
/// readable, the signature no longer fits).
pub fn tamper(apk: &[u8], entry: &str) -> Option<Vec<u8>> {
    let name = entry.as_bytes();
    let at = (0..apk.len().saturating_sub(30 + name.len())).find(|&p| {
        apk[p..p + 4] == *b"PK\x03\x04"
            && usize::from(u16::from_le_bytes([apk[p + 26], apk[p + 27]])) == name.len()
            && &apk[p + 30..p + 30 + name.len()] == name
    })?;
    let extra = usize::from(u16::from_le_bytes([apk[at + 28], apk[at + 29]]));
    let size = u32::from_le_bytes(apk[at + 18..at + 22].try_into().ok()?) as usize;
    if size == 0 {
        return None;
    }
    let mut out = apk.to_vec();
    out[at + 30 + name.len() + extra + size / 2] ^= 0x01;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apk::sig::{check, Signed};
    use crate::apk::zip::tests::build;

    fn checked(apk: &[u8]) -> Signed {
        check(&mut Archive::open(Cursor::new(apk)).unwrap())
    }

    #[test]
    fn a_signed_file_is_valid_and_names_its_signer() {
        let zip = build(
            &[("AndroidManifest.xml", b"m"), ("classes.dex", &[7u8; 5000])],
            true,
        );
        let a = resign(&zip, "Ultar Test A").unwrap();
        let Signed::Valid { scheme, signers } = checked(&a) else {
            panic!("{:?}", checked(&a));
        };
        assert_eq!(scheme, 2);
        assert_eq!(signers.len(), 1);
        assert_eq!(signers[0].1, "Ultar Test A");
        assert_eq!(signers[0].0.len(), 64);
        // signed again by someone else: another fingerprint
        let b = resign(&a, "Ultar Test B").unwrap();
        let Signed::Valid { signers: other, .. } = checked(&b) else {
            panic!();
        };
        assert_ne!(other[0].0, signers[0].0);
        assert_eq!(other[0].1, "Ultar Test B");
        // the same signer again: the same fingerprint
        let Signed::Valid { signers: again, .. } = checked(&resign(&b, "Ultar Test A").unwrap())
        else {
            panic!();
        };
        assert_eq!(again, signers);
    }

    #[test]
    fn a_changed_byte_anywhere_breaks_the_signature() {
        let zip = build(
            &[("AndroidManifest.xml", b"m"), ("classes.dex", &[7u8; 5000])],
            false,
        );
        let a = resign(&zip, "Ultar Test A").unwrap();
        assert_eq!(
            checked(&tamper(&a, "classes.dex").unwrap()),
            Signed::Invalid
        );
        // in the directory (a byte the ZIP reader ignores: version made by)
        let central = u32::from_le_bytes(a[a.len() - 6..a.len() - 2].try_into().unwrap()) as usize;
        let mut d = a.clone();
        d[central + 4] ^= 1;
        assert_eq!(checked(&d), Signed::Invalid);
        // with a ZIP comment the end record still counts whole
        let mut c = zip.clone();
        let n = c.len();
        c[n - 2..].copy_from_slice(&3u16.to_le_bytes());
        c.extend_from_slice(b"hey");
        let signed = resign(&c, "Ultar Test A").unwrap();
        assert!(matches!(checked(&signed), Signed::Valid { .. }));
        let mut changed = signed.clone();
        let n = changed.len();
        changed[n - 1] = b'!';
        assert_eq!(checked(&changed), Signed::Invalid);
    }
}
