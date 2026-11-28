//! Anti-forensic split (LUKS1 AF, hash SHA-256): the key spread over `stripes` blocks, so
//! that erasing a small part of the keyslot area destroys the key.

use crate::Error;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Each 32-byte part (and the shorter rest) becomes SHA-256(index BE32 || part).
fn diffuse(buf: &mut [u8]) {
    for (i, part) in buf.chunks_mut(32).enumerate() {
        let mut h = Sha256::new();
        h.update((i as u32).to_be_bytes());
        h.update(&*part);
        let d = h.finalize();
        let n = part.len();
        part.copy_from_slice(&d[..n]);
    }
}

fn xor(acc: &mut [u8], other: &[u8]) {
    acc.iter_mut().zip(other).for_each(|(a, b)| *a ^= b);
}

/// Splits `key` into `stripes` blocks (the first `stripes - 1` random).
pub fn split(key: &[u8], stripes: usize) -> Result<Zeroizing<Vec<u8>>, Error> {
    let n = key.len();
    let mut out = Zeroizing::new(vec![0u8; n * stripes]);
    crate::random(&mut out[..n * (stripes - 1)])?;
    let mut acc = Zeroizing::new(vec![0u8; n]);
    for i in 0..stripes - 1 {
        xor(&mut acc, &out[i * n..(i + 1) * n]);
        diffuse(&mut acc);
    }
    let last = &mut out[(stripes - 1) * n..];
    last.copy_from_slice(&acc);
    xor(last, key);
    Ok(out)
}

/// The key of `n` bytes from `data` (`stripes` blocks).
pub fn merge(data: &[u8], n: usize, stripes: usize) -> Zeroizing<Vec<u8>> {
    let mut acc = Zeroizing::new(vec![0u8; n]);
    for i in 0..stripes - 1 {
        xor(&mut acc, &data[i * n..(i + 1) * n]);
        diffuse(&mut acc);
    }
    xor(&mut acc, &data[(stripes - 1) * n..stripes * n]);
    acc
}
