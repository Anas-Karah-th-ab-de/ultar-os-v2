//! State files of the services: one value per file, written atomically and checked.
//!
//! Layout: `ULTARSTATE1\n`, the payload length and its CRC-32 as two little-endian `u32`,
//! then the payload (the value in the bus encoding). A file that is cut, too long, has a
//! wrong check sum or does not decode is reported as [`StoreError::Damaged`] – never
//! guessed at. Writing goes to a temporary file in the same directory (mode `0600`),
//! which is synced, renamed over the old one, and the directory is synced too, so a
//! power loss leaves either the old or the new file.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const MAGIC: &[u8] = b"ULTARSTATE1\n";
/// Largest payload a state file may hold.
pub const MAX_PAYLOAD: usize = 4 * 1024 * 1024;

/// Why a state file could not be read or written.
#[derive(Debug)]
pub enum StoreError {
    /// There is no file yet.
    Missing,
    /// The file exists but is not a valid state file (cut, changed, too long).
    Damaged,
    /// Reading or writing failed.
    Io(std::io::Error),
    /// The value is larger than [`MAX_PAYLOAD`] or cannot be encoded.
    TooLarge,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => f.write_str("no state file"),
            Self::Damaged => f.write_str("state file damaged"),
            Self::Io(e) => write!(f, "state file: {e}"),
            Self::TooLarge => f.write_str("state too large"),
        }
    }
}

impl std::error::Error for StoreError {}

/// Encodes a value as the bytes of a state file.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, StoreError> {
    let payload = ultar_bus::encode(value).map_err(|_| StoreError::TooLarge)?;
    if payload.len() > MAX_PAYLOAD {
        return Err(StoreError::TooLarge);
    }
    let mut out = Vec::with_capacity(MAGIC.len() + 8 + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&crc32fast::hash(&payload).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Decodes the bytes of a state file.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, StoreError> {
    let rest = bytes.strip_prefix(MAGIC).ok_or(StoreError::Damaged)?;
    let (head, payload) = rest.split_at_checked(8).ok_or(StoreError::Damaged)?;
    let word = |i: usize| -> u32 {
        let mut b = [0u8; 4];
        b.copy_from_slice(&head[i..i + 4]);
        u32::from_le_bytes(b)
    };
    let (len, crc) = (word(0) as usize, word(4));
    if len != payload.len() || len > MAX_PAYLOAD || crc32fast::hash(payload) != crc {
        return Err(StoreError::Damaged);
    }
    ultar_bus::decode(payload).map_err(|_| StoreError::Damaged)
}

/// Reads the value stored at `path`.
pub fn load<T: DeserializeOwned>(path: &Path) -> Result<T, StoreError> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(StoreError::Missing),
        Err(e) => return Err(StoreError::Io(e)),
    };
    let mut bytes = Vec::new();
    let limit = (MAGIC.len() + 8 + MAX_PAYLOAD + 1) as u64;
    file.take(limit)
        .read_to_end(&mut bytes)
        .map_err(StoreError::Io)?;
    decode(&bytes)
}

/// Stores `value` at `path` atomically (see the module documentation).
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), StoreError> {
    let bytes = encode(value)?;
    let dir = path.parent().ok_or(StoreError::Io(std::io::Error::from(
        std::io::ErrorKind::InvalidInput,
    )))?;
    let name = path.file_name().ok_or(StoreError::Io(std::io::Error::from(
        std::io::ErrorKind::InvalidInput,
    )))?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(name);
    temp_name.push(".new");
    let temp = dir.join(temp_name);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result.map_err(StoreError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("svckit-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn values_round_trip_and_files_are_private() {
        let d = dir("rt");
        let path = d.join("state");
        assert!(matches!(
            load::<Vec<String>>(&path),
            Err(StoreError::Missing)
        ));
        let value = vec!["أ".to_owned(), "b".to_owned()];
        save(&path, &value).unwrap();
        assert_eq!(load::<Vec<String>>(&path).unwrap(), value);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        save(&path, &vec!["c".to_owned()]).unwrap();
        assert_eq!(load::<Vec<String>>(&path).unwrap(), vec!["c".to_owned()]);
        assert_eq!(
            std::fs::read_dir(&d).unwrap().count(),
            1,
            "no temporary file left"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn every_cut_and_every_changed_byte_is_detected() {
        let bytes = encode(&(7u32, "calendar".to_owned())).unwrap();
        for cut in 0..bytes.len() {
            assert!(decode::<(u32, String)>(&bytes[..cut]).is_err(), "cut {cut}");
        }
        for i in 0..bytes.len() {
            let mut changed = bytes.clone();
            changed[i] ^= 0x41;
            assert!(decode::<(u32, String)>(&changed).is_err(), "byte {i}");
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(matches!(
            decode::<(u32, String)>(&longer),
            Err(StoreError::Damaged)
        ));
        assert_eq!(decode::<(u32, String)>(&bytes).unwrap().0, 7);
    }

    #[test]
    fn errors_are_reported() {
        let big = vec![0u8; MAX_PAYLOAD + 1];
        assert!(matches!(encode(&big), Err(StoreError::TooLarge)));
        assert!(matches!(save(Path::new("/"), &1u8), Err(StoreError::Io(_))));
        let missing_dir = Path::new("/nonexistent-ultar-dir/x");
        assert!(matches!(save(missing_dir, &1u8), Err(StoreError::Io(_))));
        for e in [
            StoreError::Missing,
            StoreError::Damaged,
            StoreError::TooLarge,
        ] {
            assert!(!e.to_string().is_empty());
        }
    }
}
