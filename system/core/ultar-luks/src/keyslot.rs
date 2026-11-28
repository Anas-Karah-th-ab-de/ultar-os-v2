//! One keyslot: passphrase → Argon2id (or PBKDF2) → XTS key of the keyslot area, which holds
//! the AF-split master key; the digest tells the right key from a wrong one.

use crate::{af, num, Error, Key, Params, Reader, Sectors, KEY_LEN, SECTOR, STRIPES};
use argon2::{Algorithm, Argon2, Version};
use serde_json::Value;
use sha2::Sha256;
use zeroize::Zeroizing;

/// Largest keyslot area read (a damaged header must not ask for gigabytes).
const MAX_AREA: u64 = 16 << 20;
/// Most PBKDF2 iterations accepted from a header (keeps a forged header from hanging the start).
const MAX_ITER: u32 = 50_000_000;

pub struct Sealed {
    /// Argon2id salt.
    pub salt: [u8; 32],
    /// The encrypted keyslot area (4 KiB aligned).
    pub area: Vec<u8>,
}

fn argon2id(
    pass: &[u8],
    salt: &[u8],
    memory: u32,
    time: u32,
    lanes: u32,
    algo: Algorithm,
) -> Result<Key, Error> {
    let params = argon2::Params::new(memory, time, lanes, Some(KEY_LEN)).map_err(|_| Error::Kdf)?;
    let mut out: Key = Zeroizing::new([0u8; KEY_LEN]);
    Argon2::new(algo, Version::V0x13, params)
        .hash_password_into(pass, salt, &mut out[..])
        .map_err(|_| Error::Kdf)?;
    Ok(out)
}

/// PBKDF2-HMAC-SHA-256 of the master key: the stored digest.
pub fn digest(key: &[u8], salt: &[u8], iterations: u32, len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    pbkdf2::pbkdf2_hmac::<Sha256>(key, salt, iterations, &mut out);
    out
}

/// Keyslot area for `key` under `pass`.
pub fn seal(key: &Key, pass: &[u8], p: &Params) -> Result<Sealed, Error> {
    let mut salt = [0u8; 32];
    crate::random(&mut salt)?;
    let slot_key = argon2id(
        pass,
        &salt,
        p.memory_kib,
        p.time,
        p.lanes,
        Algorithm::Argon2id,
    )?;
    let split = af::split(&key[..], STRIPES)?;
    let mut area = vec![0u8; split.len().div_ceil(4096) * 4096];
    area[..split.len()].copy_from_slice(&split);
    Sectors::new(&slot_key).encrypt(0, &mut area);
    Ok(Sealed { salt, area })
}

fn b64(v: &Value) -> Result<Vec<u8>, Error> {
    use base64::Engine;
    let s = v.as_str().ok_or(Error::Header("base64 field"))?;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|_| Error::Header("base64"))
}

fn u32_of(v: &Value, what: &'static str) -> Result<u32, Error> {
    num(v)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(Error::Header(what))
}

/// The keyslot key the passphrase gives.
fn slot_key(kdf: &Value, pass: &[u8]) -> Result<Key, Error> {
    let salt = b64(&kdf["salt"])?;
    match kdf["type"].as_str() {
        Some(t @ ("argon2id" | "argon2i")) => {
            let algo = if t == "argon2id" {
                Algorithm::Argon2id
            } else {
                Algorithm::Argon2i
            };
            let memory = u32_of(&kdf["memory"], "kdf memory")?;
            if memory > 4 << 20 {
                return Err(Error::Unsupported("kdf memory above 4 GiB"));
            }
            argon2id(
                pass,
                &salt,
                memory,
                u32_of(&kdf["time"], "kdf time")?,
                u32_of(&kdf["cpus"], "kdf cpus")?,
                algo,
            )
        }
        Some("pbkdf2") => {
            if kdf["hash"] != "sha256" {
                return Err(Error::Unsupported("pbkdf2 hash (only sha256)"));
            }
            let iter = u32_of(&kdf["iterations"], "kdf iterations")?;
            if iter > MAX_ITER {
                return Err(Error::Unsupported("pbkdf2 iterations"));
            }
            let mut out: Key = Zeroizing::new([0u8; KEY_LEN]);
            pbkdf2::pbkdf2_hmac::<Sha256>(pass, &salt, iter, &mut out[..]);
            Ok(out)
        }
        _ => Err(Error::Unsupported("kdf type")),
    }
}

/// Whether `key` matches the digest `d`.
fn matches(key: &[u8], d: &Value) -> Result<bool, Error> {
    if d["type"] != "pbkdf2" || d["hash"] != "sha256" {
        return Err(Error::Unsupported("digest (only pbkdf2 sha256)"));
    }
    let iter = u32_of(&d["iterations"], "digest iterations")?;
    if iter > MAX_ITER {
        return Err(Error::Unsupported("digest iterations"));
    }
    let want = b64(&d["digest"])?;
    let got = digest(key, &b64(&d["salt"])?, iter, want.len());
    Ok(!want.is_empty() && got.iter().zip(&want).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0)
}

/// Opens keyslot `s`: `Some(key)` when the passphrase is right.
pub fn open(
    s: &Value,
    d: &Value,
    read: &mut Reader<'_>,
    pass: &[u8],
) -> Result<Option<Key>, Error> {
    if s["type"] != "luks2" || num(&s["key_size"]) != Some(KEY_LEN as u64) {
        return Err(Error::Unsupported("keyslot type or key size"));
    }
    let af = &s["af"];
    if af["type"] != "luks1" || af["hash"] != "sha256" {
        return Err(Error::Unsupported("AF (only luks1 sha256)"));
    }
    let stripes = num(&af["stripes"])
        .filter(|&n| n > 0)
        .ok_or(Error::Header("stripes"))? as usize;
    let area = &s["area"];
    if area["type"] != "raw"
        || area["encryption"] != "aes-xts-plain64"
        || num(&area["key_size"]) != Some(KEY_LEN as u64)
    {
        return Err(Error::Unsupported("keyslot area cipher"));
    }
    let offset = num(&area["offset"]).ok_or(Error::Header("area offset"))?;
    let need = (KEY_LEN * stripes) as u64;
    let size = need.div_ceil(SECTOR) * SECTOR;
    if size > MAX_AREA || num(&area["size"]).is_none_or(|n| n < size) {
        return Err(Error::Header("area size"));
    }
    let key = slot_key(&s["kdf"], pass)?;
    let mut buf = Zeroizing::new(vec![0u8; size as usize]);
    read(offset, &mut buf)?;
    Sectors::new(&key).decrypt(0, &mut buf);
    let merged = af::merge(&buf, KEY_LEN, stripes);
    if !matches(&merged, d)? {
        return Ok(None);
    }
    let mut out: Key = Zeroizing::new([0u8; KEY_LEN]);
    out.copy_from_slice(&merged);
    Ok(Some(out))
}
