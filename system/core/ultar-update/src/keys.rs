//! Update trust keys: the set burned into the system image, and rotations accepted since.
//!
//! The update keys are separate from the app-package keys. A rotation replaces the whole set and
//! must be signed by a key of the set in force; its serial only grows (see
//! `docs/SYSTEM-UPDATE.md` §2.3).

use crate::error::{Malformed, UpdateError};
use crate::fsutil::{read_bounded, write_atomic};
use crate::monotonic::MonotonicStore;
use crate::sign::{self, PublicKey, KEYS_DOMAIN, KEYS_MAGIC};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Most keys in a set.
pub const MAX_KEYS: usize = 8;
/// Largest rotation body in bytes.
pub const MAX_ROTATION: usize = 1024;
/// Most rotations kept on the device. When the chain is full, the links that the burned-in key
/// set of the running image already covers (serial not above the burned-in serial) are dropped
/// to make room; only if none can be dropped is a further rotation refused.
pub const MAX_CHAIN: usize = 64;
const MAX_CHAIN_BYTES: u64 = (MAX_CHAIN * (MAX_ROTATION + sign::ENVELOPE_OVERHEAD)) as u64;
const MAX_KEY_FILE: u64 = 4096;
const HEADER: &str = "ultar-update-keys 1";
/// File-name ending of public key files in the burned-in directory.
pub const KEY_FILE_SUFFIX: &str = ".ed25519";

/// A set of trusted public keys with its serial.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeySet {
    /// Serial of the set; a larger serial replaces a smaller one.
    pub serial: u64,
    /// The keys, sorted and unique.
    pub keys: Vec<PublicKey>,
}

/// A key rotation document: the complete new set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rotation {
    /// The new set.
    pub set: KeySet,
    /// When it was signed (seconds since 1970).
    pub created: u64,
}

fn key_from_hex(text: &str) -> Option<PublicKey> {
    let mut key = [0u8; 32];
    hex::decode_to_slice(text, &mut key).ok()?;
    Some(key)
}

impl Rotation {
    /// The canonical bytes. Fails for an empty, oversized, unsorted or duplicate key list.
    pub fn encode(&self) -> Result<Vec<u8>, Malformed> {
        let keys = &self.set.keys;
        let sorted = keys.windows(2).all(|w| w[0] < w[1]);
        if keys.is_empty() || keys.len() > MAX_KEYS || !sorted {
            return Err(Malformed::Field);
        }
        let mut out = format!(
            "{HEADER}\nserial={}\ncreated={}\n",
            self.set.serial, self.created
        );
        for key in keys {
            let _ = writeln!(out, "key={}", hex::encode(key));
        }
        Ok(out.into_bytes())
    }

    /// Parses canonical rotation bytes (parse, encode again, compare).
    pub fn parse(bytes: &[u8]) -> Result<Self, Malformed> {
        if bytes.len() > MAX_ROTATION {
            return Err(Malformed::Length);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| Malformed::Text)?;
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return Err(Malformed::Magic);
        }
        let mut number = |name: &str| -> Result<u64, Malformed> {
            let line = lines.next().ok_or(Malformed::Text)?;
            let value = line.strip_prefix(name).ok_or(Malformed::Text)?;
            value.parse().map_err(|_| Malformed::Field)
        };
        let serial = number("serial=")?;
        let created = number("created=")?;
        let mut keys = Vec::new();
        for line in lines {
            let hex = line.strip_prefix("key=").ok_or(Malformed::Text)?;
            if keys.len() == MAX_KEYS {
                return Err(Malformed::Length);
            }
            keys.push(key_from_hex(hex).ok_or(Malformed::Field)?);
        }
        let rotation = Self {
            set: KeySet { serial, keys },
            created,
        };
        if rotation.encode()? != bytes {
            return Err(Malformed::Text);
        }
        Ok(rotation)
    }

    /// Signs the rotation with `secret` (which must belong to the set in force on the devices).
    pub fn sign(&self, secret: &[u8; 32]) -> Result<Vec<u8>, Malformed> {
        sign::seal(&KEYS_MAGIC, KEYS_DOMAIN, &self.encode()?, secret).ok_or(Malformed::Length)
    }
}

/// Reads one key file: a hex key per line; empty lines and `#` comments are skipped.
/// Any other line is an error (a damaged trust file must not silently shrink the set).
pub fn read_key_file(path: &Path) -> Result<Vec<PublicKey>, UpdateError> {
    let bytes = read_bounded(path, MAX_KEY_FILE)
        .map_err(|_| UpdateError::Malformed(Malformed::Length))?
        .ok_or(UpdateError::Malformed(Malformed::Length))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Malformed::Text)?;
    let mut keys = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        keys.push(key_from_hex(line).ok_or(Malformed::Field)?);
    }
    Ok(keys)
}

/// Reads a secret key file: 64 hex characters (the 32-byte seed), surrounding space ignored.
pub fn read_secret_file(path: &Path) -> Result<[u8; 32], UpdateError> {
    Ok(read_key_file(path)?
        .first()
        .copied()
        .ok_or(Malformed::Field)?)
}

/// Loads the burned-in set from `dir`: every `*.ed25519` file, plus the optional file `serial`.
/// A directory that does not exist gives an empty set (nothing will verify).
pub fn load_burned(dir: &Path) -> Result<KeySet, UpdateError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(KeySet::default()),
        Err(e) => return Err(e.into()),
    };
    let mut keys = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with(KEY_FILE_SUFFIX) {
            keys.extend(read_key_file(&path)?);
        }
    }
    keys.sort_unstable();
    keys.dedup();
    if keys.len() > MAX_KEYS {
        return Err(Malformed::Length.into());
    }
    let serial = match read_bounded(&dir.join("serial"), 32)? {
        None => 0,
        Some(bytes) => std::str::from_utf8(&bytes)
            .ok()
            .and_then(|t| t.trim().parse().ok())
            .ok_or(Malformed::Field)?,
    };
    Ok(KeySet { serial, keys })
}

fn link_serial(link: &[u8]) -> Option<u64> {
    let envelope = sign::open(&KEYS_MAGIC, link, MAX_ROTATION).ok()?;
    Some(Rotation::parse(envelope.body).ok()?.set.serial)
}

/// The trust state of the device: burned-in keys plus the verified chain of rotations.
#[derive(Debug)]
pub struct TrustStore {
    chain_path: PathBuf,
    serial: MonotonicStore,
    chain: Vec<Vec<u8>>,
    current: KeySet,
    burned_serial: u64,
}

impl TrustStore {
    /// Opens the trust state. `burned_dir` is the read-only key directory of the system image,
    /// `state_dir` the updater's state directory. The stored chain is verified link by link;
    /// a chain that is damaged or shorter than the stored serial is
    /// [`UpdateError::StateCorrupt`].
    pub fn open(burned_dir: &Path, state_dir: &Path) -> Result<Self, UpdateError> {
        let chain_path = state_dir.join("trust").join("chain");
        let serial = MonotonicStore::new(&state_dir.join("state"), "keyserial");
        let mut current = load_burned(burned_dir)?;
        let burned_serial = current.serial;
        let stored = read_bounded(&chain_path, MAX_CHAIN_BYTES)
            .map_err(|_| UpdateError::StateCorrupt)?
            .unwrap_or_default();
        let mut chain = Vec::new();
        let mut rest = stored.as_slice();
        while !rest.is_empty() {
            let envelope = sign::open(&KEYS_MAGIC, rest, MAX_ROTATION)
                .map_err(|_| UpdateError::StateCorrupt)?;
            let rotation = Rotation::parse(envelope.body).map_err(|_| UpdateError::StateCorrupt)?;
            if rotation.set.serial > current.serial {
                envelope
                    .verify(KEYS_DOMAIN, &current.keys)
                    .map_err(|_| UpdateError::StateCorrupt)?;
                current = rotation.set;
            }
            chain.push(rest[..envelope.len].to_vec());
            rest = &rest[envelope.len..];
        }
        if current.serial < serial.get()? {
            return Err(UpdateError::StateCorrupt);
        }
        Ok(Self {
            chain_path,
            serial,
            chain,
            current,
            burned_serial,
        })
    }

    /// The keys in force.
    pub fn keys(&self) -> &[PublicKey] {
        &self.current.keys
    }

    /// Serial of the set in force.
    pub fn serial(&self) -> u64 {
        self.current.serial
    }

    /// Offers a signed rotation. `Ok(true)`: verified, stored and in force. `Ok(false)`: its
    /// serial is not newer than the set in force, nothing changed (old rotations inside a
    /// bundle are normal). Errors: not signed by the set in force, malformed, chain full.
    pub fn accept(&mut self, signed: &[u8]) -> Result<bool, UpdateError> {
        let envelope = sign::open_exact(&KEYS_MAGIC, signed, MAX_ROTATION)?;
        let rotation = Rotation::parse(envelope.body)?;
        if rotation.set.serial <= self.current.serial {
            return Ok(false);
        }
        envelope.verify(KEYS_DOMAIN, &self.current.keys)?;
        let mut chain = self.chain.clone();
        if chain.len() >= MAX_CHAIN {
            // Full: links the running image's own key set makes unnecessary can go. (They
            // are kept as long as there is room, so that a fall-back to an older image with
            // an older burned-in set still finds its way through the chain.)
            let burned = self.burned_serial;
            chain.retain(|link| link_serial(link).is_some_and(|serial| serial > burned));
        }
        if chain.len() >= MAX_CHAIN {
            return Err(UpdateError::RotationRefused);
        }
        chain.push(signed.to_vec());
        write_atomic(&self.chain_path, &chain.concat())?;
        self.serial.raise(rotation.set.serial)?;
        self.chain = chain;
        self.current = rotation.set;
        Ok(true)
    }
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
