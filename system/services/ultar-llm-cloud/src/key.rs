//! The person's key: one file in the service's folder, mode 0600.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use ultar_bus::messages::llm_cloud::MAX_KEY;

/// Where the key is kept.
#[derive(Debug, Clone)]
pub struct KeyStore {
    file: PathBuf,
}

/// A key as people paste it: no spaces around it, printable, not empty, not too long.
pub fn clean(key: &str) -> Option<&str> {
    let key = key.trim();
    let printable = key.bytes().all(|b| b.is_ascii_graphic());
    (!key.is_empty() && key.len() <= MAX_KEY && printable).then_some(key)
}

impl KeyStore {
    /// The store in `dir`.
    pub fn new(dir: &Path) -> Self {
        Self {
            file: dir.join("key"),
        }
    }

    /// The key, when one is stored.
    pub fn read(&self) -> Option<String> {
        let text = std::fs::read_to_string(&self.file).ok()?;
        clean(&text).map(str::to_owned)
    }

    /// Stores `key` (written beside and moved into place, never world-readable).
    pub fn write(&self, key: &str) -> bool {
        let Some(key) = clean(key) else {
            return false;
        };
        let part = self.file.with_extension("part");
        let _ = std::fs::remove_file(&part);
        let mut options = std::fs::OpenOptions::new();
        let written = options
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&part)
            .and_then(|mut f| f.write_all(key.as_bytes()).and_then(|()| f.sync_all()));
        written.is_ok() && std::fs::rename(&part, &self.file).is_ok()
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

    #[test]
    fn a_key_is_kept_for_this_service_only_and_can_be_forgotten() {
        let dir = std::env::temp_dir().join(format!("llmcloud-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = KeyStore::new(&dir);
        assert_eq!(store.read(), None);
        assert!(store.write("  sk-test-123\n"));
        assert_eq!(store.read().as_deref(), Some("sk-test-123"));
        let mode = std::fs::metadata(dir.join("key"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(store.write("sk-other"));
        assert_eq!(store.read().as_deref(), Some("sk-other"));
        assert!(store.clear() && store.clear());
        assert_eq!(store.read(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_is_no_key_is_not_stored() {
        assert_eq!(clean(""), None);
        assert_eq!(clean("   "), None);
        assert_eq!(clean("two words"), None);
        assert_eq!(clean("line\nbreak"), None);
        assert_eq!(clean(&"k".repeat(MAX_KEY + 1)), None);
        assert_eq!(clean(" abc "), Some("abc"));
    }
}
