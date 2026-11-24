//! The developer's own signing key, kept in the app's data area (`app:data`).
//!
//! Created once with mode `0600` (never wider, not even for a moment), one hex line. The key
//! signs only the developer's own packages; a device trusts it only after an administrator put
//! the public key into its trusted keys as `<publisher>.ed25519`.
//!
//! Note: APP-RULES §5 wants secrets in the keyring service; there is no such service yet, so
//! the key stays in the app's private data area (owner-only, removed with the app).

use crate::error::AppError;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// File name of the key inside the data area.
pub const KEY_FILE: &str = "developer.key";

/// The developer key: private half for signing, public half to hand to administrators.
#[derive(Clone)]
pub struct DevKey {
    private_hex: String,
    /// Public key, hex (the content of a trusted-keys file).
    pub public_hex: String,
    /// Key id as packages carry it (first 16 hex of sha256(public key)).
    pub id: String,
}

impl std::fmt::Debug for DevKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the private half.
        f.debug_struct("DevKey").field("id", &self.id).finish()
    }
}

impl DevKey {
    /// The private key in hex for `ultar_pkg::pack_and_sign`.
    pub fn private_hex(&self) -> &str {
        &self.private_hex
    }

    fn from_private(private_hex: String) -> Result<DevKey, AppError> {
        let bytes = hex::decode(private_hex.trim()).map_err(|_| AppError::Key)?;
        let arr: [u8; 32] = bytes.as_slice().try_into().map_err(|_| AppError::Key)?;
        let public = ed25519_dalek::SigningKey::from_bytes(&arr).verifying_key();
        Ok(DevKey {
            private_hex: private_hex.trim().to_string(),
            public_hex: hex::encode(public.as_bytes()),
            id: ultar_pkg::key_id(&public),
        })
    }
}

/// Path of the key file in `data_dir`.
pub fn key_path(data_dir: &Path) -> PathBuf {
    data_dir.join(KEY_FILE)
}

/// Loads the key from `data_dir`, creating it on first use.
pub fn load_or_create(data_dir: &Path) -> Result<DevKey, AppError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let path = key_path(data_dir);
    match std::fs::symlink_metadata(&path) {
        Ok(meta) => {
            if !meta.is_file() || meta.mode() & 0o077 != 0 || meta.len() > 256 {
                return Err(AppError::Key);
            }
            let mut text = String::new();
            std::fs::File::open(&path)
                .and_then(|f| f.take(256).read_to_string(&mut text))
                .map_err(|_| AppError::Key)?;
            DevKey::from_private(text)
        }
        Err(_) => {
            std::fs::create_dir_all(data_dir).map_err(|_| AppError::Key)?;
            let (private_hex, _) = ultar_pkg::keygen();
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .map_err(|_| AppError::Key)?;
            file.write_all(format!("{private_hex}\n").as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|_| AppError::Key)?;
            DevKey::from_private(private_hex)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn created_once_with_owner_only_mode_and_reloaded() {
        let dir = tempfile::tempdir().unwrap();
        let a = load_or_create(dir.path()).unwrap();
        let mode = std::fs::metadata(key_path(dir.path()))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let b = load_or_create(dir.path()).unwrap();
        assert_eq!(a.public_hex, b.public_hex);
        assert_eq!(a.id.len(), 16);
        assert!(!format!("{a:?}").contains(a.private_hex()));
    }

    #[test]
    fn refuses_a_key_readable_by_others_or_damaged() {
        let dir = tempfile::tempdir().unwrap();
        load_or_create(dir.path()).unwrap();
        let path = key_path(dir.path());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(load_or_create(dir.path()).unwrap_err(), AppError::Key);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&path, "not hex").unwrap();
        assert_eq!(load_or_create(dir.path()).unwrap_err(), AppError::Key);
    }
}
