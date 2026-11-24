//! The three signing keys and how they are stored.
//!
//! A key file holds one line: the 32-byte Ed25519 private key in hex. It must be a regular file
//! (no symlink) that nobody but its owner can read or write (`mode & 0o077 == 0`), owned by the
//! user the service runs as. The public half is derived, never stored next to it.

use crate::error::SignError;
use ed25519_dalek::{SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

/// Which key signs what. Separate keys keep a stolen store key from signing a system update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Ultar OS release images and installers (`*.img`, `*.zip`), SSHSIG namespace `ultar-release`.
    Os,
    /// System apps (`.uap` installed with the `app.system` profile; device: `trusted-keys/system/`).
    System,
    /// Store apps and the store catalogue (`app.standard` profile; device: `trusted-keys/`).
    Store,
}

impl Role {
    /// Every role.
    pub const ALL: [Role; 3] = [Role::Os, Role::System, Role::Store];

    /// File name of the role's key in the key directory.
    pub const fn file(self) -> &'static str {
        match self {
            Self::Os => "os.key",
            Self::System => "system.key",
            Self::Store => "store.key",
        }
    }

    /// The word used on the wire and in the audit log.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Os => "os",
            Self::System => "system",
            Self::Store => "store",
        }
    }

    /// Parses [`Role::name`].
    pub fn parse(word: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.name() == word)
    }
}

/// Key id as used by `.uap` packages and the catalogue: first 16 hex of sha256(public key).
pub fn key_id(public: &VerifyingKey) -> String {
    let mut id = hex::encode(Sha256::digest(public.as_bytes()));
    id.truncate(16);
    id
}

/// Mode bits that must be clear on a key file and the key directory.
const FOREIGN_BITS: u32 = 0o077;
/// Largest key file read.
const MAX_KEY_FILE: u64 = 256;

/// Loads the key of `role` from `dir`, checking type, size and permissions first.
pub fn load(dir: &Path, role: Role) -> Result<SigningKey, SignError> {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    let dir_meta = std::fs::symlink_metadata(dir).map_err(|_| SignError::KeyMissing)?;
    if !dir_meta.is_dir() || dir_meta.mode() & FOREIGN_BITS != 0 {
        return Err(SignError::KeyPermissions);
    }
    let path = dir.join(role.file());
    let meta = std::fs::symlink_metadata(&path).map_err(|_| SignError::KeyMissing)?;
    if !meta.is_file() || meta.mode() & FOREIGN_BITS != 0 || meta.uid() != dir_meta.uid() {
        return Err(SignError::KeyPermissions);
    }
    if meta.len() > MAX_KEY_FILE {
        return Err(SignError::KeyDamaged);
    }
    let mut text = Zeroizing::new(String::new());
    std::fs::File::open(&path)
        .and_then(|f| f.take(MAX_KEY_FILE).read_to_string(&mut text))
        .map_err(|_| SignError::KeyMissing)?;
    parse_private(text.trim())
}

fn parse_private(hex_text: &str) -> Result<SigningKey, SignError> {
    let bytes = Zeroizing::new(hex::decode(hex_text).map_err(|_| SignError::KeyDamaged)?);
    let arr: Zeroizing<[u8; 32]> = Zeroizing::new(
        bytes
            .as_slice()
            .try_into()
            .map_err(|_| SignError::KeyDamaged)?,
    );
    Ok(SigningKey::from_bytes(&arr))
}

/// Creates a new key for `role` in `dir` (refuses to overwrite). The file is created with
/// mode `0400` from the start (no window with a wider mode). Answers the public key.
pub fn generate(dir: &Path, role: Role) -> Result<VerifyingKey, SignError> {
    use std::os::unix::fs::OpenOptionsExt;
    let key = SigningKey::generate(&mut rand_core::OsRng);
    let path: PathBuf = dir.join(role.file());
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .open(&path)?;
    let text = Zeroizing::new(format!("{}\n", hex::encode(key.to_bytes())));
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    Ok(key.verifying_key())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn keydir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    #[test]
    fn generate_then_load_gives_the_same_key_and_mode_0400() {
        let dir = keydir();
        let public = generate(dir.path(), Role::Store).unwrap();
        let mode = std::fs::metadata(dir.path().join("store.key"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o400);
        assert_eq!(
            load(dir.path(), Role::Store).unwrap().verifying_key(),
            public
        );
        assert_eq!(key_id(&public).len(), 16);
        // Never overwrites.
        assert_eq!(generate(dir.path(), Role::Store), Err(SignError::Io));
    }

    #[test]
    fn refuses_wide_permissions_symlinks_and_damage() {
        let dir = keydir();
        generate(dir.path(), Role::Os).unwrap();
        let path = dir.path().join("os.key");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o440)).unwrap();
        assert_eq!(load(dir.path(), Role::Os), Err(SignError::KeyPermissions));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(load(dir.path(), Role::Os).is_ok());
        // Directory open to others.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(load(dir.path(), Role::Os), Err(SignError::KeyPermissions));
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        // Symlink in place of the file.
        std::os::unix::fs::symlink(&path, dir.path().join("system.key")).unwrap();
        assert_eq!(
            load(dir.path(), Role::System),
            Err(SignError::KeyPermissions)
        );
        // Missing.
        assert_eq!(load(dir.path(), Role::Store), Err(SignError::KeyMissing));
        // Damaged contents.
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "zz\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
        assert_eq!(load(dir.path(), Role::Os), Err(SignError::KeyDamaged));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&path, "ab".repeat(200)).unwrap();
        assert_eq!(load(dir.path(), Role::Os), Err(SignError::KeyDamaged));
    }

    #[test]
    fn role_names_round_trip() {
        for r in Role::ALL {
            assert_eq!(Role::parse(r.name()), Some(r));
        }
        assert_eq!(Role::parse("root"), None);
    }
}
