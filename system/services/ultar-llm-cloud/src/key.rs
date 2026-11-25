//! The person's key at rest: sealed, never in the clear.
//!
//! The key file holds `version ‖ nonce ‖ AES-256-GCM(key)`. The sealing key is derived
//! (HMAC-SHA256) from a random secret of this installation that lies in **another** folder
//! (`--seal`), so the key file alone – in a backup, a support bundle, a copied folder – is
//! worthless. What this cannot do: protect against someone who reads both folders or runs
//! as this service on the running device; that needs a hardware key store, which this
//! system does not have yet.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use ultar_bus::messages::llm_cloud::MAX_KEY;

const VERSION: u8 = 1;
const NONCE: usize = 12;
const SECRET: usize = 32;
const PURPOSE: &[u8] = b"ultar-llm-cloud key v1";

/// Where the key is kept.
#[derive(Debug, Clone)]
pub struct KeyStore {
    file: PathBuf,
    secret: PathBuf,
}

/// A key as people paste it: no spaces around it, printable, not empty, not too long.
pub fn clean(key: &str) -> Option<&str> {
    let key = key.trim();
    let printable = key.bytes().all(|b| b.is_ascii_graphic());
    (!key.is_empty() && key.len() <= MAX_KEY && printable).then_some(key)
}

fn write_private(file: &Path, bytes: &[u8]) -> bool {
    let part = file.with_extension("part");
    let _ = std::fs::remove_file(&part);
    let mut options = std::fs::OpenOptions::new();
    let written = options
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&part)
        .and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_all()));
    written.is_ok() && std::fs::rename(&part, file).is_ok()
}

impl KeyStore {
    /// The store: the sealed key in `dir`, the installation's secret in `seal`.
    pub fn new(dir: &Path, seal: &Path) -> Self {
        Self {
            file: dir.join("key"),
            secret: seal.join("secret"),
        }
    }

    /// The installation's secret; made on first use when `create`.
    fn secret(&self, create: bool) -> Option<[u8; SECRET]> {
        if let Ok(bytes) = std::fs::read(&self.secret) {
            return bytes.try_into().ok();
        }
        if !create {
            return None;
        }
        let mut secret = [0u8; SECRET];
        rand::rngs::OsRng.fill_bytes(&mut secret);
        write_private(&self.secret, &secret).then_some(secret)
    }

    fn cipher(secret: &[u8; SECRET]) -> Option<Aes256Gcm> {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret).ok()?;
        mac.update(PURPOSE);
        Aes256Gcm::new_from_slice(&mac.finalize().into_bytes()).ok()
    }

    /// The key, when one is stored and this installation can open it.
    pub fn read(&self) -> Option<String> {
        let sealed = std::fs::read(&self.file).ok()?;
        let (version, rest) = sealed.split_first()?;
        if *version != VERSION || rest.len() <= NONCE {
            return None;
        }
        let (nonce, text) = rest.split_at(NONCE);
        let cipher = Self::cipher(&self.secret(false)?)?;
        let sealed = Payload {
            msg: text,
            aad: PURPOSE,
        };
        let clear = cipher.decrypt(Nonce::from_slice(nonce), sealed).ok()?;
        let key = String::from_utf8(clear).ok()?;
        clean(&key).map(str::to_owned)
    }

    /// Whether a key file exists (it may be one this installation cannot open).
    pub fn present(&self) -> bool {
        self.file.exists()
    }

    /// Seals and stores `key` (written beside and moved into place).
    pub fn write(&self, key: &str) -> bool {
        let Some(key) = clean(key) else {
            return false;
        };
        let Some(cipher) = self.secret(true).as_ref().and_then(Self::cipher) else {
            return false;
        };
        let mut nonce = [0u8; NONCE];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let clear = Payload {
            msg: key.as_bytes(),
            aad: PURPOSE,
        };
        let Ok(text) = cipher.encrypt(Nonce::from_slice(&nonce), clear) else {
            return false;
        };
        let sealed = [&[VERSION][..], &nonce, &text].concat();
        write_private(&self.file, &sealed)
    }

    /// Forgets the key; `true` when none is left.
    pub fn clear(&self) -> bool {
        match std::fs::remove_file(&self.file) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn store(tag: &str) -> (KeyStore, PathBuf) {
        let root = std::env::temp_dir().join(format!("llmcloud-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (dir, seal) = (root.join("state"), root.join("seal"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&seal).unwrap();
        (KeyStore::new(&dir, &seal), root)
    }

    #[test]
    fn the_key_is_sealed_and_only_this_installation_opens_it() {
        let (store, root) = store("seal");
        assert_eq!(store.read(), None);
        assert!(store.write("  sk-test-123\n"));
        assert_eq!(store.read().as_deref(), Some("sk-test-123"));
        let file = root.join("state/key");
        let sealed = std::fs::read(&file).unwrap();
        let hay = String::from_utf8_lossy(&sealed).into_owned();
        assert!(!hay.contains("sk-test"), "never in the clear");
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        // sealed anew each time: the same key gives another file
        assert!(store.write("sk-test-123"));
        assert_ne!(std::fs::read(&file).unwrap(), sealed);

        // the key file without the secret (a copied folder) opens nothing
        let secret = root.join("seal/secret");
        let saved = std::fs::read(&secret).unwrap();
        std::fs::remove_file(&secret).unwrap();
        assert_eq!(store.read(), None);
        assert!(store.present());
        // another installation's secret, or a changed file, opens nothing either
        std::fs::write(&secret, [7u8; 32]).unwrap();
        assert_eq!(store.read(), None);
        std::fs::write(&secret, &saved).unwrap();
        assert_eq!(store.read().as_deref(), Some("sk-test-123"));
        let mut broken = std::fs::read(&file).unwrap();
        let last = broken.len() - 1;
        broken[last] ^= 1;
        std::fs::write(&file, &broken).unwrap();
        assert_eq!(store.read(), None);

        assert!(store.clear() && store.clear());
        assert!(!store.present());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn what_is_no_key_is_not_stored() {
        assert_eq!(clean(""), None);
        assert_eq!(clean("   "), None);
        assert_eq!(clean("two words"), None);
        assert_eq!(clean("line\nbreak"), None);
        assert_eq!(clean(&"k".repeat(MAX_KEY + 1)), None);
        assert_eq!(clean(" abc "), Some("abc"));
        let (store, root) = store("none");
        assert!(!store.write("two words"));
        assert!(!store.present());
        let _ = std::fs::remove_dir_all(&root);
    }
}
