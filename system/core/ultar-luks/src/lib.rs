//! # ultar-luks – LUKS2 data encryption of Ultar OS
//!
//! Writes and opens LUKS2 volumes in the on-disk format of cryptsetup (one keyslot, Argon2id,
//! AF split with SHA-256, PBKDF2-SHA-256 digest, segment `aes-xts-plain64` with 512-byte
//! sectors). Rule "keine eigene Krypto": every primitive comes from a reviewed RustCrypto
//! crate (`aes`, `xts-mode`, `argon2`, `pbkdf2`, `sha2`); this crate only lays out the bytes.
//!
//! Who uses it:
//! - the installer formats the data partition ([`format`]) and writes the copied data through
//!   [`Sectors`] behind [`DATA_OFFSET`];
//! - `ultar-luks unlock` (started by ultar-init before the services) asks for the passphrase,
//!   opens the volume ([`open`]) and hands the master key to the kernel, which decrypts the
//!   partition itself (`kernel/ultar-kern/src/crypt.rs`). The kernel never sees the passphrase.

mod af;
mod header;
mod keyslot;
mod sectors;
mod start;

pub use sectors::Sectors;
pub use start::{start_info, StartInfo, TOKEN_TYPE};

use serde_json::{json, Value};
use zeroize::Zeroizing;

/// Sector size of the data segment and of the keyslot area.
pub const SECTOR: u64 = 512;
/// Master key length: two AES-256 keys for XTS.
pub const KEY_LEN: usize = 64;
/// Size of one header copy (binary header plus JSON area), the cryptsetup default.
pub const HDR_SIZE: u64 = 16384;
/// Start of the keyslot area (behind both header copies).
pub const KEYSLOTS_OFFSET: u64 = 2 * HDR_SIZE;
/// Start of the encrypted data (cryptsetup default: 16 MiB).
pub const DATA_OFFSET: u64 = 16 << 20;
/// Size of the keyslot area.
pub const KEYSLOTS_SIZE: u64 = DATA_OFFSET - KEYSLOTS_OFFSET;
/// AF stripes of a keyslot (the LUKS standard value).
pub const STRIPES: usize = 4000;
/// Magic of the primary header.
pub const MAGIC: [u8; 6] = *b"LUKS\xba\xbe";

/// The master key (zeroed when dropped).
pub type Key = Zeroizing<[u8; KEY_LEN]>;

/// Cost of the key derivation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Params {
    /// Argon2id memory in KiB.
    pub memory_kib: u32,
    /// Argon2id passes (at least 4, the cryptsetup minimum).
    pub time: u32,
    /// Argon2id lanes.
    pub lanes: u32,
    /// PBKDF2 iterations of the key digest.
    pub digest_iterations: u32,
}

impl Params {
    /// Installed systems: 256 MiB, 4 passes, 4 lanes (about a second on a current PC).
    pub const INSTALL: Params = Params {
        memory_kib: 256 * 1024,
        time: 4,
        lanes: 4,
        digest_iterations: 100_000,
    };
    /// Tests and test images only: cheap, still valid for cryptsetup.
    pub const TEST: Params = Params {
        memory_kib: 1024,
        time: 4,
        lanes: 1,
        digest_iterations: 1000,
    };
}

/// Why a volume could not be written or opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No LUKS2 header at the start.
    NotLuks,
    /// Header or metadata damaged (what).
    Header(&'static str),
    /// No keyslot opens with this passphrase.
    Passphrase,
    /// A feature this crate does not handle (what).
    Unsupported(&'static str),
    /// Reading the device failed.
    Io(std::io::ErrorKind),
    /// The random source failed.
    Random,
    /// The key derivation refused its parameters.
    Kdf,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotLuks => write!(f, "no LUKS2 header"),
            Error::Header(w) => write!(f, "damaged LUKS2 metadata: {w}"),
            Error::Passphrase => write!(f, "wrong passphrase"),
            Error::Unsupported(w) => write!(f, "unsupported LUKS2 feature: {w}"),
            Error::Io(k) => write!(f, "read failed: {k:?}"),
            Error::Random => write!(f, "random source failed"),
            Error::Kdf => write!(f, "key derivation failed"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e.kind())
    }
}

pub(crate) fn random(buf: &mut [u8]) -> Result<(), Error> {
    getrandom::getrandom(buf).map_err(|_| Error::Random)
}

/// Whether `head` (at least 8 bytes of the partition start) begins a LUKS2 header.
pub fn is_luks(head: &[u8]) -> bool {
    head.len() >= 8 && head[..6] == MAGIC && head[6..8] == [0, 2]
}

/// A freshly formatted volume.
pub struct Formatted {
    /// Bytes for the partition start: both headers and the keyslot area (the rest up to
    /// [`DATA_OFFSET`] is to be zeroed).
    pub metadata: Vec<u8>,
    /// The new master key (encrypts the data behind [`DATA_OFFSET`]).
    pub key: Key,
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn uuid() -> Result<String, Error> {
    let mut b = [0u8; 16];
    random(&mut b)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    ))
}

/// The JSON metadata of a volume with one keyslot and one segment.
fn metadata_json(
    slot: &keyslot::Sealed,
    params: &Params,
    digest_salt: &[u8],
    digest: &[u8],
    tokens: Value,
) -> String {
    let json_size = HDR_SIZE - 4096;
    let meta = json!({
        "keyslots": { "0": {
            "type": "luks2",
            "key_size": KEY_LEN,
            "af": { "type": "luks1", "stripes": STRIPES, "hash": "sha256" },
            "area": {
                "type": "raw",
                "offset": KEYSLOTS_OFFSET.to_string(),
                "size": slot.area.len().to_string(),
                "encryption": "aes-xts-plain64",
                "key_size": KEY_LEN,
            },
            "kdf": {
                "type": "argon2id",
                "time": params.time,
                "memory": params.memory_kib,
                "cpus": params.lanes,
                "salt": b64(&slot.salt),
            },
        }},
        "tokens": tokens,
        "segments": { "0": {
            "type": "crypt",
            "offset": DATA_OFFSET.to_string(),
            "size": "dynamic",
            "iv_tweak": "0",
            "encryption": "aes-xts-plain64",
            "sector_size": SECTOR,
        }},
        "digests": { "0": {
            "type": "pbkdf2",
            "keyslots": ["0"],
            "segments": ["0"],
            "hash": "sha256",
            "iterations": params.digest_iterations,
            "salt": b64(digest_salt),
            "digest": b64(digest),
        }},
        "config": { "json_size": json_size.to_string(), "keyslots_size": KEYSLOTS_SIZE.to_string() },
    });
    meta.to_string()
}

/// Formats a new volume: a random master key in keyslot 0, protected by `passphrase`.
pub fn format(passphrase: &[u8], params: &Params) -> Result<Formatted, Error> {
    format_with(passphrase, params, None)
}

/// [`format`] with the start information for the unlock screen in an `ultar-start` token.
pub fn format_with(
    passphrase: &[u8],
    params: &Params,
    start: Option<&StartInfo>,
) -> Result<Formatted, Error> {
    let mut key: Key = Zeroizing::new([0u8; KEY_LEN]);
    random(&mut key[..])?;
    let slot = keyslot::seal(&key, passphrase, params)?;
    let mut digest_salt = [0u8; 32];
    random(&mut digest_salt)?;
    let digest = keyslot::digest(&key[..], &digest_salt, params.digest_iterations, 32);
    let tokens = match start {
        Some(s) => json!({ "0": s.token() }),
        None => json!({}),
    };
    let text = metadata_json(&slot, params, &digest_salt, &digest, tokens);
    let id = uuid()?;
    let mut salt = [0u8; 64];
    random(&mut salt)?;
    let mut metadata = vec![0u8; KEYSLOTS_OFFSET as usize + slot.area.len()];
    for (at, magic) in [(0u64, MAGIC), (HDR_SIZE, header::MAGIC2)] {
        let h = &mut metadata[at as usize..(at + HDR_SIZE) as usize];
        header::write(
            h,
            &header::Fields {
                magic,
                seqid: 1,
                offset: at,
                uuid: &id,
                salt: &salt,
            },
            &text,
        )?;
    }
    metadata[KEYSLOTS_OFFSET as usize..].copy_from_slice(&slot.area);
    Ok(Formatted { metadata, key })
}

/// An opened volume.
pub struct Opened {
    /// The master key.
    pub key: Key,
    /// Byte offset of the encrypted data in the partition.
    pub offset: u64,
}

/// Reads `buf.len()` bytes at a byte offset of the partition.
pub type Reader<'a> = dyn FnMut(u64, &mut [u8]) -> Result<(), Error> + 'a;

/// A number that LUKS2 JSON keeps either as string (64-bit values) or as number.
pub(crate) fn num(v: &Value) -> Option<u64> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

/// The data segment: must be the one this crate and the kernel decrypt.
fn segment(meta: &Value) -> Result<u64, Error> {
    let segs = meta["segments"]
        .as_object()
        .ok_or(Error::Header("segments"))?;
    if segs.len() != 1 {
        return Err(Error::Unsupported("more than one segment"));
    }
    let s = segs.values().next().ok_or(Error::Header("segments"))?;
    if s["type"] != "crypt" || s["encryption"] != "aes-xts-plain64" {
        return Err(Error::Unsupported("segment cipher (only aes-xts-plain64)"));
    }
    if num(&s["sector_size"]) != Some(SECTOR) {
        return Err(Error::Unsupported("segment sector size (only 512)"));
    }
    if num(&s["iv_tweak"]).unwrap_or(0) != 0 {
        return Err(Error::Unsupported("segment iv_tweak"));
    }
    if s["size"] != "dynamic" {
        return Err(Error::Unsupported("fixed segment size"));
    }
    if s.get("integrity").is_some()
        || s.get("flags")
            .and_then(Value::as_array)
            .is_some_and(|f| !f.is_empty())
    {
        return Err(Error::Unsupported("segment flags or integrity"));
    }
    if meta["config"].get("requirements").is_some() {
        return Err(Error::Unsupported("requirements (reencryption)"));
    }
    let offset = num(&s["offset"]).ok_or(Error::Header("segment offset"))?;
    if offset % SECTOR != 0 || offset < KEYSLOTS_OFFSET {
        return Err(Error::Header("segment offset"));
    }
    Ok(offset)
}

/// Opens a volume with `passphrase`; `read` gives bytes of the partition.
pub fn open(read: &mut Reader<'_>, passphrase: &[u8]) -> Result<Opened, Error> {
    let meta = header::read_best(read)?;
    let offset = segment(&meta)?;
    let slots = meta["keyslots"]
        .as_object()
        .ok_or(Error::Header("keyslots"))?;
    let digests = meta["digests"]
        .as_object()
        .ok_or(Error::Header("digests"))?;
    let mut tried = false;
    for (id, slot) in slots {
        let Some(d) = digests.values().find(|d| {
            d["keyslots"]
                .as_array()
                .is_some_and(|k| k.iter().any(|x| x == id))
        }) else {
            continue;
        };
        tried = true;
        if let Some(key) = keyslot::open(slot, d, read, passphrase)? {
            return Ok(Opened { key, offset });
        }
    }
    if tried {
        Err(Error::Passphrase)
    } else {
        Err(Error::Unsupported("no usable keyslot"))
    }
}

/// [`open`] on the bytes of the partition start (at least up to the end of the keyslots).
pub fn open_bytes(meta: &[u8], passphrase: &[u8]) -> Result<Opened, Error> {
    open(
        &mut |at, buf| {
            let at = at as usize;
            let src = meta
                .get(at..at + buf.len())
                .ok_or(Error::Header("short metadata"))?;
            buf.copy_from_slice(src);
            Ok(())
        },
        passphrase,
    )
}

#[cfg(test)]
mod tests;
