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

/// The key id of a public key (first 16 hex of sha256(pubkey), as `.uap`).
pub fn key_id(pubkey: &VerifyingKey) -> String {
    let mut h = Sha256::new();
    h.update(pubkey.as_bytes());
    hex::encode(h.finalize())[..16].to_string()
}

/// Signs a catalogue body, returning the detached signature blob. The private
/// key never lives in the running app; this is for the publisher tool and the
/// stick-preparation tests.
pub fn sign_catalog(body: &[u8], signing: &ed25519_dalek::SigningKey) -> String {
    use ed25519_dalek::Signer;
    let msg = catalog_message(body);
    let sig = signing.sign(&msg);
    signature_blob(&signing.verifying_key(), &sig)
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
    fn message_binds_the_domain() {
        let m = catalog_message(b"body");
        let text = String::from_utf8(m.clone()).unwrap_or_default();
        assert!(text.starts_with("ultar-catalog-v1\n"));
        assert!(text.ends_with('\n'));
        // magic + sha256 + trailing newline = two lines, domain-bound.
        assert_eq!(text.lines().count(), 2);
        // The package message over the same body differs: different magic.
        let mut h = Sha256::new();
        h.update(b"body");
        let pkg = format!("ultar-pkg-v1\n{}\n", hex::encode(h.finalize())).into_bytes();
        assert_ne!(m, pkg);
    }

    #[test]
    fn sign_verify_round_trip_and_tamper() {
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let body = b"format_version = 1\n";
        let blob = sign_catalog(body, &signing);
        let trusted = unique_dir("keys");
        std::fs::create_dir_all(&trusted).unwrap();
        std::fs::write(
            trusted.join("vendor.ed25519"),
            hex::encode(signing.verifying_key().to_bytes()) + "\n",
        )
        .unwrap();
        assert_eq!(
            verify_signature(body, &blob, &trusted).unwrap(),
            Some(key_id(&signing.verifying_key()))
        );
        // Tampered body: the same blob no longer verifies.
        assert_eq!(
            verify_signature(b"format_version = 2\n", &blob, &trusted),
            Err(SourceError::UnknownSigner)
        );
        // Unknown signer: nothing trusted in that directory.
        let other = unique_dir("other-keys");
        std::fs::create_dir_all(&other).unwrap();
        assert_eq!(
            verify_signature(body, &blob, &other),
            Err(SourceError::UnknownSigner)
        );
        std::fs::remove_dir_all(&trusted).unwrap();
        std::fs::remove_dir_all(&other).unwrap();
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
        assert_eq!(loaded.dir.as_deref(), Some(dir.display().to_string().as_str()));
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
