//! Where a catalogue comes from (DESKTOP-SPEC §7.3 "Offline-Quelle").
//!
//! Local sources never contact the internet; the optional online source
//! ([`crate::online`], feature `online`) fetches the same signed files over HTTPS and keeps a
//! verified copy, so it always falls back to them. A catalogue is a TOML body plus a detached
//! Ed25519 signature, verified against the trusted keys of the device.
//!
//! The data model, the signature check and the file source ([`FileCatalogSource`]) live in the
//! shared crate `ultar-catalog` (`system/core/ultar-catalog`), which the shell's overview search
//! uses too; this module re-exports them and adds the Store's own sources: the [`CatalogSource`]
//! trait (with package download), the embedded image catalogue and the stick discovery.

use crate::catalog::Catalog;
use crate::error::SourceError;
use std::path::{Path, PathBuf};

pub use crate::signature::{
    catalog_message, key_id, sign_catalog, signature_blob, verify_signature, CATALOG_MAGIC,
};
pub use ultar_catalog::source::{
    trusted_dir_from_env, FileCatalogSource, LoadedCatalog, CATALOG_FILE, CATALOG_SIG_SUFFIX,
    MAX_CATALOG_BYTES,
};

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

/// The shared file source is one of the Store's catalogue sources (local files are already
/// present, so `ensure_package` does nothing).
impl CatalogSource for FileCatalogSource {
    fn name(&self) -> String {
        FileCatalogSource::name(self)
    }

    fn load(&self) -> Result<LoadedCatalog, SourceError> {
        FileCatalogSource::load(self)
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
