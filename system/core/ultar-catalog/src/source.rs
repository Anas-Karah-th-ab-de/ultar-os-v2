//! Reading a catalogue from a directory: the TOML body plus its detached signature,
//! verified against the device's trusted keys before a byte is parsed (DESKTOP-SPEC §7.3
//! "Offline-Quelle"). Local sources never contact the internet.
//!
//! A catalogue is a TOML body plus a detached Ed25519 signature ([`crate::sign`]), verified
//! against the trusted keys of the device — the same key directory and the same signature blob
//! layout as `.uap` packages (`ultar-pkg`), with its own domain prefix so a package signature
//! is never a catalogue signature.

use crate::model::Catalog;
use crate::sign::verify_signature;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Largest catalogue body read from a source (a stick is untrusted until verified).
pub const MAX_CATALOG_BYTES: u64 = 4 * 1024 * 1024;
/// Largest signature blob read from a source.
const MAX_SIG_BYTES: u64 = 4096;
/// Name of the catalogue body inside a source directory.
pub const CATALOG_FILE: &str = "ultar-catalog.toml";
/// Name of its signature (the body name plus this suffix).
pub const CATALOG_SIG_SUFFIX: &str = ".sig";

/// What can go wrong on a catalogue source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// The catalogue file could not be read (missing, denied).
    Unreadable,
    /// The catalogue body is not valid TOML or does not follow the format.
    Damaged,
    /// There is no signature file next to the catalogue.
    Unsigned,
    /// The signature does not match the body.
    BadSignature,
    /// The signature is well-formed but no trusted key matches it.
    UnknownSigner,
    /// The catalogue format version is newer than this app understands.
    TooNew,
    /// A downloaded package does not match the size or SHA-256 the signed catalogue pins
    /// (or the catalogue pins none for an online package); it was discarded.
    PackageMismatch,
}

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

/// A catalogue read from `<dir>/ultar-catalog.toml` + `.sig` and verified against
/// `<trusted_dir>` (a folder, a mounted USB stick, the Store's verified cache, …).
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

    /// The directory this source reads.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Human-readable name of the source (the directory's last word).
    pub fn name(&self) -> String {
        self.dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.dir.display().to_string())
    }

    /// Writes a new signed catalogue into `dir` (used to prepare a stick; the private
    /// key never lives in the app — it is passed in by the publisher tool).
    pub fn write_signed(dir: &Path, body: &str, blob: &str) -> Result<PathBuf, SourceError> {
        std::fs::create_dir_all(dir).map_err(|_| SourceError::Unreadable)?;
        let file = dir.join(CATALOG_FILE);
        std::fs::write(&file, body.as_bytes()).map_err(|_| SourceError::Unreadable)?;
        std::fs::write(
            dir.join(format!("{CATALOG_FILE}{CATALOG_SIG_SUFFIX}")),
            blob,
        )
        .map_err(|_| SourceError::Unreadable)?;
        Ok(file)
    }

    /// Loads and parses the catalogue, verifying the signature first.
    pub fn load(&self) -> Result<LoadedCatalog, SourceError> {
        // Read once into memory: the bytes that are verified are the bytes
        // that are parsed (no second read of the file, no TOCTOU).
        let body = read_capped(&self.dir.join(CATALOG_FILE), MAX_CATALOG_BYTES)
            .map_err(|_| SourceError::Unreadable)?
            .ok_or(SourceError::Damaged)?;
        let blob = read_capped(
            &self.dir.join(format!("{CATALOG_FILE}{CATALOG_SIG_SUFFIX}")),
            MAX_SIG_BYTES,
        )
        .map_err(|_| SourceError::Unsigned)?
        .ok_or(SourceError::BadSignature)?;
        let blob = String::from_utf8(blob).map_err(|_| SourceError::BadSignature)?;
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

/// Reads at most `max` bytes; `Ok(None)` when the file is larger.
fn read_capped(path: &Path, max: u64) -> std::io::Result<Option<Vec<u8>>> {
    let file = std::fs::File::open(path)?;
    let mut out = Vec::new();
    file.take(max + 1).read_to_end(&mut out)?;
    Ok((out.len() as u64 <= max).then_some(out))
}

/// The trusted-keys directory; `ULTAR_TRUSTED` overrides (tests, dev images).
pub fn trusted_dir_from_env() -> PathBuf {
    std::env::var("ULTAR_TRUSTED")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/ultar/trusted-keys"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sign::sign_catalog;
    use ed25519_dalek::SigningKey;

    fn unique_dir(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "ultar-catalog-test-{}-{}-{}",
            tag,
            std::process::id(),
            {
                use std::sync::atomic::{AtomicU32, Ordering};
                static N: AtomicU32 = AtomicU32::new(0);
                N.fetch_add(1, Ordering::SeqCst)
            }
        ));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn file_source_loads_signed_and_reports_missing_signature() {
        let signing = SigningKey::from_bytes(&[9u8; 32]);
        let body = "format_version = 1\n\n[[app]]\nid = \"media\"\ncategory = \"media\"\nicon = \"media\"\npublisher = \"p\"\nsource = \"ultar\"\n[app.name]\nar = \"الوسائط\"\nen = \"Media\"\n[app.summary]\nar = \"s\"\nen = \"s\"\n[app.description]\nar = \"d\"\nen = \"d\"\n";
        let dir = unique_dir("src");
        let keys = unique_dir("keys");
        std::fs::create_dir_all(&keys).unwrap();
        std::fs::write(
            keys.join("v.ed25519"),
            hex::encode(signing.verifying_key().to_bytes()) + "\n",
        )
        .unwrap();
        let blob = sign_catalog(body.as_bytes(), &signing);
        FileCatalogSource::write_signed(&dir, body, &blob).unwrap();
        let src = FileCatalogSource::new(&dir, &keys);
        let loaded = src.load().unwrap();
        assert!(loaded.verified);
        assert_eq!(loaded.catalog.apps.len(), 1);
        assert_eq!(
            loaded.dir.as_deref(),
            Some(dir.display().to_string().as_str())
        );
        // Unsigned stick: the body exists, the .sig does not.
        std::fs::remove_file(dir.join(format!("{CATALOG_FILE}{CATALOG_SIG_SUFFIX}"))).unwrap();
        assert_eq!(src.load(), Err(SourceError::Unsigned));
        // Missing directory.
        let missing = FileCatalogSource::new(dir.join("nope"), &keys);
        assert_eq!(missing.load(), Err(SourceError::Unreadable));
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&keys).unwrap();
    }
}
