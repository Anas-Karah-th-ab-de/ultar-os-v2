//! Where a catalogue comes from (DESKTOP-SPEC §7.3 "Offline-Quelle").
//!
//! Local sources never contact the internet; the optional online source
//! ([`crate::online`], feature `online`) fetches the same signed files over HTTPS and keeps a
//! verified copy, so it always falls back to them. A catalogue is a TOML body plus a detached
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
use std::io::Read;
use std::path::{Path, PathBuf};

pub use crate::signature::{
    catalog_message, key_id, sign_catalog, signature_blob, verify_signature, CATALOG_MAGIC,
};

/// Largest catalogue body read from a source (a stick is untrusted until verified).
pub const MAX_CATALOG_BYTES: u64 = 4 * 1024 * 1024;
/// Largest signature blob read from a source.
const MAX_SIG_BYTES: u64 = 4096;
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

    /// Makes sure the package file at `path` (built from [`LoadedCatalog::dir`] and the entry's
    /// `package`) is present before it goes to the launcher. Local sources have every file
    /// already; the online source downloads it and checks it against the signed catalogue.
    fn ensure_package(&self, _path: &Path) -> Result<(), SourceError> {
        Ok(())
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
        std::fs::write(
            dir.join(format!("{CATALOG_FILE}{CATALOG_SIG_SUFFIX}")),
            blob,
        )
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
    use ed25519_dalek::SigningKey;

    fn unique_dir(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "ultar-store-test-{}-{}-{}",
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

    #[test]
    fn embedded_source_parses_the_built_in_catalog() {
        let src = EmbeddedCatalogSource::new("image", crate::BUILT_IN_CATALOG);
        let loaded = src.load().unwrap();
        assert!(loaded.verified);
        assert_eq!(loaded.catalog.apps.len(), 12);
    }

    #[test]
    fn find_sources_lists_sticks_with_a_catalog_and_package_files() {
        let media = unique_dir("media");
        let stick = media.join("USB");
        std::fs::create_dir_all(&stick).unwrap();
        std::fs::write(stick.join(CATALOG_FILE), "x").unwrap();
        std::fs::write(stick.join(format!("{CATALOG_FILE}.sig")), "y").unwrap();
        std::fs::write(stick.join("tool.uap"), "z").unwrap();
        std::fs::create_dir_all(media.join("EMPTY")).unwrap();
        let found = find_sources(&media);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "USB");
        assert!(found[0].has_catalog);
        let files = package_files(&stick);
        assert_eq!(files, vec!["tool.uap".to_string()]);
        std::fs::remove_dir_all(&media).unwrap();
    }
}
