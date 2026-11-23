//! Where the device key lives. Callers get the public key and signatures, never the private key.
//!
//! [`KeyStore`] is the whole interface: `public_key()` and `sign(message)`. [`FileKeyStore`]
//! implements it with a file for hardware without a secure element; a TPM or secure-element store
//! implements the same two calls and replaces it without any change elsewhere.
//!
//! The key file is created without ever replacing an existing file (temporary file, flush, hard
//! link, directory flush), has mode 0600, and is refused on every use unless it is a regular file
//! owned by the calling user with no access for anyone else and an intact check line.

use crate::identity::{PUBLIC_KEY_BYTES, SIGNATURE_BYTES};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const HEADER: &str = "ultar-device-key-v1";
/// Largest key file read (a valid one has about 230 bytes).
const MAX_FILE_BYTES: u64 = 1024;

/// Why the key could not be used.
#[derive(Debug)]
pub enum KeyStoreError {
    /// Reading or writing the store failed.
    Io(std::io::Error),
    /// The key file is not a private regular file of the calling user; the text names the reason.
    Insecure(&'static str),
    /// The key file does not have the expected content (damaged or tampered with).
    Corrupt,
    /// The operating system gave no random bytes.
    Random,
}

impl fmt::Display for KeyStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyStoreError::Io(e) => write!(f, "key store i/o: {e}"),
            KeyStoreError::Insecure(why) => write!(f, "key file refused: {why}"),
            KeyStoreError::Corrupt => f.write_str("key file is damaged"),
            KeyStoreError::Random => f.write_str("no random bytes available"),
        }
    }
}

impl std::error::Error for KeyStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            KeyStoreError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for KeyStoreError {
    fn from(e: std::io::Error) -> Self {
        KeyStoreError::Io(e)
    }
}

/// A place that holds the device's Ed25519 key and signs with it.
pub trait KeyStore {
    /// The public key.
    fn public_key(&self) -> Result<[u8; PUBLIC_KEY_BYTES], KeyStoreError>;
    /// An Ed25519 signature over `message`. The private key does not leave the store.
    fn sign(&self, message: &[u8]) -> Result<[u8; SIGNATURE_BYTES], KeyStoreError>;
}

/// The device key in a private file.
#[derive(Debug, Clone)]
pub struct FileKeyStore {
    path: PathBuf,
}

fn check_line(seed: &[u8; 32], public: &[u8; 32]) -> String {
    let mut h = Sha256::new();
    h.update(HEADER.as_bytes());
    h.update(seed);
    h.update(public);
    hex::encode(h.finalize())
}

fn encode_file(key: &SigningKey) -> String {
    let (seed, public) = (key.to_bytes(), key.verifying_key().to_bytes());
    format!(
        "{HEADER}\nseed={}\npublic={}\ncheck={}\n",
        hex::encode(seed),
        hex::encode(public),
        check_line(&seed, &public)
    )
}

fn hex32(line: Option<&str>, name: &str) -> Result<[u8; 32], KeyStoreError> {
    let value = line
        .and_then(|l| l.strip_prefix(name))
        .and_then(|l| l.strip_prefix('='))
        .ok_or(KeyStoreError::Corrupt)?;
    let bytes = hex::decode(value).map_err(|_| KeyStoreError::Corrupt)?;
    <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| KeyStoreError::Corrupt)
}

fn decode_file(text: &str) -> Result<SigningKey, KeyStoreError> {
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err(KeyStoreError::Corrupt);
    }
    let seed = hex32(lines.next(), "seed")?;
    let public = hex32(lines.next(), "public")?;
    let check = hex32(lines.next(), "check")?;
    let key = SigningKey::from_bytes(&seed);
    let intact = lines.next().is_none()
        && key.verifying_key().to_bytes() == public
        && hex::encode(check) == check_line(&seed, &public);
    if intact {
        Ok(key)
    } else {
        Err(KeyStoreError::Corrupt)
    }
}

#[cfg(unix)]
fn private_to_caller(meta: &std::fs::Metadata) -> Result<(), KeyStoreError> {
    use std::os::unix::fs::MetadataExt;
    if meta.uid() != rustix::process::geteuid().as_raw() {
        return Err(KeyStoreError::Insecure("owned by another user"));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(KeyStoreError::Insecure("readable or writable by others"));
    }
    Ok(())
}

#[cfg(not(unix))]
fn private_to_caller(_: &std::fs::Metadata) -> Result<(), KeyStoreError> {
    Err(KeyStoreError::Insecure("no ownership check on this system"))
}

#[cfg(unix)]
fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(not(unix))]
fn same_file(_: &std::fs::Metadata, _: &std::fs::Metadata) -> bool {
    false
}

impl FileKeyStore {
    /// A store at `path`. Nothing is read or written yet.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The store at `path`, with a freshly generated key if there was none. An existing file is
    /// never replaced: if it is damaged or not private the error is returned and the file stays.
    pub fn open_or_create(path: impl Into<PathBuf>) -> Result<Self, KeyStoreError> {
        let store = Self::new(path);
        if !store.exists() {
            store.create()?;
        }
        store.load()?;
        Ok(store)
    }

    /// The file of this store.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// True when something exists at the path (a file, a link, anything).
    pub fn exists(&self) -> bool {
        std::fs::symlink_metadata(&self.path).is_ok()
    }

    /// Generates a key and stores it, unless a file is already there (then nothing changes: the
    /// other writer's key wins and is used).
    fn create(&self) -> Result<(), KeyStoreError> {
        use rand_core::RngCore;
        let mut seed = [0u8; 32];
        rand_core::OsRng
            .try_fill_bytes(&mut seed)
            .map_err(|_| KeyStoreError::Random)?;
        let content = encode_file(&SigningKey::from_bytes(&seed));
        let parent = self.path.parent().filter(|p| !p.as_os_str().is_empty());
        if let Some(dir) = parent {
            std::fs::create_dir_all(dir)?;
        }
        // A name of its own for every attempt: concurrent creators never touch each other's file.
        let mut nonce = [0u8; 8];
        rand_core::OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| KeyStoreError::Random)?;
        let mut tmp_name = self.path.as_os_str().to_os_string();
        tmp_name.push(format!(".new.{}", hex::encode(nonce)));
        let tmp = PathBuf::from(tmp_name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        // A hard link fails if the target exists: the key file appears complete or not at all,
        // and an existing key is never overwritten.
        let linked = file
            .write_all(content.as_bytes())
            .and_then(|()| file.sync_all())
            .and_then(|()| std::fs::hard_link(&tmp, &self.path));
        let _ = std::fs::remove_file(&tmp);
        match linked {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        if let Some(dir) = parent {
            std::fs::File::open(dir)?.sync_all()?;
        }
        Ok(())
    }

    fn load(&self) -> Result<SigningKey, KeyStoreError> {
        let named = std::fs::symlink_metadata(&self.path)?;
        if !named.file_type().is_file() {
            return Err(KeyStoreError::Insecure("not a regular file"));
        }
        let mut file = std::fs::File::open(&self.path)?;
        let opened = file.metadata()?;
        // The checks are made on the opened file, and it must be the file the name pointed to.
        if !opened.is_file() || !same_file(&named, &opened) {
            return Err(KeyStoreError::Insecure("changed while opening"));
        }
        private_to_caller(&opened)?;
        if opened.len() > MAX_FILE_BYTES {
            return Err(KeyStoreError::Corrupt);
        }
        let mut text = String::new();
        Read::by_ref(&mut file)
            .take(MAX_FILE_BYTES)
            .read_to_string(&mut text)
            .map_err(|_| KeyStoreError::Corrupt)?;
        decode_file(&text)
    }

    /// Deletes the key. The device then has no identity; the next use creates a new key (a new
    /// device id and, after registration, a new number). Only for an explicit reset by the user.
    pub fn destroy(&self) -> Result<(), KeyStoreError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

impl KeyStore for FileKeyStore {
    fn public_key(&self) -> Result<[u8; PUBLIC_KEY_BYTES], KeyStoreError> {
        Ok(self.load()?.verifying_key().to_bytes())
    }

    fn sign(&self, message: &[u8]) -> Result<[u8; SIGNATURE_BYTES], KeyStoreError> {
        Ok(self.load()?.sign(message).to_bytes())
    }
}

#[cfg(test)]
#[path = "keystore_tests.rs"]
mod tests;
