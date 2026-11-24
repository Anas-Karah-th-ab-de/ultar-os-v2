//! Where a catalogue comes from (DESKTOP-SPEC §7.3 "Offline-Quelle").
//!
//! The app never contacts the internet. A catalogue is a TOML body plus a detached
//! Ed25519 signature, verified against the trusted keys of the device — the same key
//! directory and the same signature blob layout as `.uap` packages (`ultar-pkg`),
//! with its own domain prefix so a package signature is never a catalogue signature:
//!
//! ```text
//! message = "ultar-catalog-v1\n" + hex(sha256(body)) + "\n"
//! file <body>.sig = "<key-id>\n" + base64(64-byte signature)
//! ```

use crate::catalog::Catalog;
use crate::error::SourceError;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Prefix of the signed message; keeps catalogue signatures out of the package domain.
pub const CATALOG_MAGIC: &str = "ultar-catalog-v1";
/// Name of the catalogue body inside a source directory.
pub const CATALOG_FILE: &str = "ultar-catalog.toml";
/// Name of its signature (the body name plus this suffix).
pub const CATALOG_SIG_SUFFIX: &str = ".sig";

/// A loaded catalogue and who signed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedCatalog {
    /// The data.
    pub catalog: Catalog,
    /// True when the signature verified against a trusted key.
    pub verified: bool,
    /// Key id of the signer (first hex word of the signature blob), if any.
    pub signer: Option<String>,
    /// Where the catalogue was read from; package files sit next to it.
    pub dir: Option<String>,
}

/// A source of catalogues: the built-in one, a folder, a USB stick, or a test double.
///
/// Implementations must never block on the network (there is none) and must classify
/// failures as [`SourceError`] so the model can show offline/error correctly.
pub trait CatalogSource: Send + Sync {
    /// Human-readable name of the source (data, e.g. a volume label).
    fn name(&self) -> String;
    /// Loads and parses the catalogue.
    fn load(&self) -> Result<LoadedCatalog, SourceError>;
}

/// The canonical message that is signed over a catalogue body.
pub fn catalog_message(body: &[u8]) -> Vec<u8>
{
    let mut h = Sha256::new();
    h.update(body);
    format!("{CATALOG_MAGIC}\n{}\n", hex::encode(h.finalize())).into_bytes()
}

/// Signature blob text for a body and signature: `"<key-id>\n<base64 sig>"`.
pub fn signature_blob(pubkey: &VerifyingKey, sig: &Signature) -> String {
    let mut h = Sha256::new();
    h.update(pubkey.as_bytes());
    let kid = hex::encode(h.finalize())[..16].to_string();
    format!("{kid}\n{}", B64.encode(sig.to_bytes()))
}

/// Reads the hex public keys of a trusted-keys directory (same files as `ultar-pkg`).
fn trusted_keys(dir: &Path) -> Vec<VerifyingKey> {
    let mut keys = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return keys;
    };
    for entry in rd.flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(bytes) = hex::decode(line) {
                if let Ok(arr) = <[u8; 32]>::try_from(bytes.as_slice()) {
                    if let Ok(k) = VerifyingKey::from_bytes(&arr) {
                        keys.push(k);
                    }
                }
            }
        }
    }
    keys
}

/// Verifies a detached signature over `body` against the trusted keys.
pub fn verify_signature(
    body: &[u8],
    blob: &str,
    trusted_dir: &Path,
) -> Result<Option<String>, SourceError> {
    let sig_b64 = blob.lines().nth(1).unwrap_or("").trim();
    let key_id = blob.lines().next().unwrap_or("").trim().to_string();
    let bytes = B64.decode(sig_b64).map_err(|_| SourceError::BadSignature)?;
    let arr: [u8; 64] = <[u8; 64]>::try_from(bytes.as_slice())
        .map_err(|_| SourceError::BadSignature)?;
    let sig = Signature::from_bytes(&arr);
    let msg = catalog_message(body);
    let keys = trusted_keys(trusted_dir);
    if keys.is_empty() {
        return Err(SourceError::UnknownSigner);
    }
    if keys.iter().any(|k| k.verify(&msg, &sig).is_ok()) {
        Ok(Some(key_id))
    } else {
        Err(SourceError::UnknownSigner)
    }
}

/// A catalogue read from `<dir>/ultar-catalog.toml` + `.sig` and verified against
/// `<trusted_dir>` (a folder, a mounted USB stick, …).
pub struct FileCatalogSource {
    dir: PathBuf,
    trusted_dir: PathBuf,
}

impl FileCatalogSource {
    /// A source reading the given directory.
    pub fn new(dir: impl Into<PathBuf>, trusted_dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            trusted_dir: trusted_dir.into(),
        }
    }

    /// Writes a new signed catalogue into `dir` (used to prepare a stick; the private
    /// key never lives in the app — it is passed in by the publisher tool).
    pub fn write_signed(dir: &Path, body: &str, blob: &str) -> Result<PathBuf, SourceError> {
        std::fs::create_dir_all(dir).map_err(|_| SourceError::Unreadable)?;
        let file = dir.join(CATALOG_FILE);
        std::fs::write(&file, body.as_bytes()).map_err(|_| SourceError::Unreadable)?;
        std::fs::write(dir.join(format!("{CATALOG_FILE}{CATALOG_SIG_SUFFIX}")), blob)
            .map_err(|_| SourceError::Unreadable)?;
        Ok(file)
    }
}

impl CatalogSource for FileCatalogSource {
    fn name(&self) -> String {
        self.dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.dir.display().to_string())
    }

    fn load(&self) -> Result<LoadedCatalog, SourceError> {
        let body = std::fs::read(self.dir.join(CATALOG_FILE)).map_err(|_| SourceError::Unreadable)?;
        let blob = std::fs::read_to_string(self.dir.join(format!("{CATALOG_FILE}{CATALOG_SIG_SUFFIX}")))
            .map_err(|_| SourceError::Unsigned)?;
        let signer = verify_signature(&body, blob.trim(), &self.trusted_dir)?;
        let text = String::from_utf8(body).map_err(|_| SourceError::Damaged)?;
        let catalog = Catalog::parse(&text)?;
        Ok(LoadedCatalog {
            catalog,
            verified: signer.is_some(),
            signer,
            dir: Some(self.dir.display().to_string()),
        })
    }
}

/// The catalogue that ships in the system image (read-only, trusted by being signed
/// into the image; loaded from the compiled-in file).
pub struct EmbeddedCatalogSource {
    name: &'static str,
    body: &'static str,
}

impl EmbeddedCatalogSource {
    /// The built-in source of the device catalogue.
    pub const fn new(name: &'static str, body: &'static str) -> Self {
        Self { name, body }
    }
}

impl CatalogSource for EmbeddedCatalogSource {
    fn name(&self) -> String {
        self.name.to_string()
    }

    fn load(&self) -> Result<LoadedCatalog, SourceError> {
        let catalog = Catalog::parse(self.body)?;
        Ok(LoadedCatalog {
            catalog,
            verified: true,
            signer: None,
            dir: None,
        })
    }
}

/// The trusted-keys directory; `ULTAR_TRUSTED` overrides (tests, dev images).
pub fn trusted_dir_from_env() -> PathBuf {
    std::env::var("ULTAR_TRUSTED")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/ultar/trusted-keys"))
}

/// One source found on the machine for the "Offline sources" page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundSource {
    /// Volume or folder name.
    pub name: String,
    /// Where to read it from.
    pub dir: PathBuf,
    /// Whether a signed catalogue was found there.
    pub has_catalog: bool,
}

/// Package files (`.uap`) sitting in a source directory, by name (shown on
/// the "Install from a file" page; the launcher re-verifies every signature).
pub fn package_files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".uap") {
            out.push(name);
        }
    }
    out.sort();
    out
}

/// Lists removable media carrying a catalogue: every subdirectory of `media_dir`
/// that holds [`CATALOG_FILE`] (the mounted stick layout).
pub fn find_sources(media_dir: &Path) -> Vec<FoundSource> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(media_dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let dir = entry.path();
        if dir.join(CATALOG_FILE).is_file() {
            let has_catalog = dir
                .join(format!("{CATALOG_FILE}{CATALOG_SIG_SUFFIX}"))
                .is_file();
            out.push(FoundSource {
                name: entry.file_name().to_string_lossy().into_owned(),
                dir,
                has_catalog,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_binds_the_domain() {
        let m = catalog_message(b"body");
        let text = String::from_utf8(m).unwrap_or_default();
        assert!(text.starts_with("ultar-catalog-v1\n"));
        assert!(text.ends_with('\n'));
        // magic + sha256 + trailing newline = two lines, domain-bound.
        assert_eq!(text.lines().count(), 2);
    }
}
