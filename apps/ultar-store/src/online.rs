//! The online catalogue source: the store server (`https://ultar.code-craft.tech/store/v1`).
//!
//! The server publishes exactly the layout of a USB stick – `ultar-catalog.toml`, its `.sig`
//! and the `.uap` files next to them – as static files. Nothing the server says is trusted:
//!
//! * the catalogue is verified with [`crate::signature::verify_signature`] against the device's
//!   trusted keys, **exactly like the file source**, before a byte of it is parsed or cached;
//! * a verified copy is kept in a cache directory (written atomically); when the server cannot
//!   be reached the cache is loaded again – and verified again – through [`FileCatalogSource`],
//!   and without a cache the image catalogue (`fallback`) is used: **offline first**;
//! * a package is only downloaded for an entry of the last verified catalogue, only when that
//!   entry pins its SHA-256, at most `size_download` (or [`MAX_PACKAGE_BYTES`]) bytes, into a
//!   `.part` file that is renamed only after the digest matched. The launcher then verifies the
//!   package signature itself – the catalogue never replaces the package gate.
//!
//! Transport is HTTPS only ([`Fetch`]; the real client is `UreqFetch` behind the feature
//! `online`: rustls with the pure-Rust Graviola provider and the Mozilla roots, no redirects,
//! no proxy from the environment, fixed timeouts).

use crate::catalog::Catalog;
use crate::error::SourceError;
use crate::signature::verify_signature;
use crate::source::{
    CatalogSource, FileCatalogSource, LoadedCatalog, CATALOG_FILE, CATALOG_SIG_SUFFIX,
    MAX_CATALOG_BYTES,
};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The store server of KM Code Craft.
pub const DEFAULT_URL: &str = "https://ultar.code-craft.tech/store/v1";
/// Largest package downloaded when the catalogue names no size.
pub const MAX_PACKAGE_BYTES: u64 = 1024 * 1024 * 1024;
/// Largest signature file read.
const MAX_SIG_BYTES: u64 = 4096;
/// Longest base URL accepted.
const MAX_URL: usize = 2048;

/// Why a download failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchError {
    /// No connection, TLS failure, timeout (the device is offline as far as the store knows).
    Unreachable,
    /// The server answered with this HTTP status.
    Status(u16),
    /// The body is larger than allowed.
    TooLarge,
    /// The local file could not be written.
    Io,
}

/// An HTTPS `GET` into a sink, at most `max` bytes (a test double replaces it in tests).
pub trait Fetch: Send + Sync {
    /// Streams the body of `url` into `sink`; answers the number of bytes.
    fn get(&self, url: &str, max: u64, sink: &mut dyn Write) -> Result<u64, FetchError>;
}

/// True for an `https://` URL without whitespace or control characters.
pub fn valid_base_url(url: &str) -> bool {
    url.starts_with("https://")
        && url.len() <= MAX_URL
        && !url
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
}

/// The online source (see the module documentation).
pub struct OnlineCatalogSource {
    base: String,
    cache: PathBuf,
    trusted: PathBuf,
    fetch: Box<dyn Fetch>,
    fallback: Arc<dyn CatalogSource>,
    last: Mutex<Option<Catalog>>,
}

impl OnlineCatalogSource {
    /// A source for `base_url` (must be `https://`), caching in `cache`, verifying against
    /// `trusted`, and falling back to `fallback` when neither server nor cache can be used.
    pub fn new(
        base_url: &str,
        cache: impl Into<PathBuf>,
        trusted: impl Into<PathBuf>,
        fetch: Box<dyn Fetch>,
        fallback: Arc<dyn CatalogSource>,
    ) -> Result<Self, SourceError> {
        let base = base_url.trim_end_matches('/');
        if !valid_base_url(base) {
            return Err(SourceError::Unreadable);
        }
        Ok(Self {
            base: base.to_string(),
            cache: cache.into(),
            trusted: trusted.into(),
            fetch,
            fallback,
            last: Mutex::new(None),
        })
    }

    fn fetch_small(&self, name: &str, max: u64) -> Result<Vec<u8>, FetchError> {
        let mut out = Vec::new();
        self.fetch
            .get(&format!("{}/{name}", self.base), max, &mut out)?;
        Ok(out)
    }

    fn remember(&self, catalog: &Catalog) {
        if let Ok(mut last) = self.last.lock() {
            *last = Some(catalog.clone());
        }
    }

    /// Fetches and verifies; `Ok(None)` when the server cannot be reached.
    fn load_remote(&self) -> Result<Option<LoadedCatalog>, SourceError> {
        let sig_name = format!("{CATALOG_FILE}{CATALOG_SIG_SUFFIX}");
        let body = match self.fetch_small(CATALOG_FILE, MAX_CATALOG_BYTES) {
            Ok(b) => b,
            Err(FetchError::TooLarge) => return Err(SourceError::Damaged),
            Err(_) => return Ok(None),
        };
        let blob = match self.fetch_small(&sig_name, MAX_SIG_BYTES) {
            Ok(b) => b,
            Err(FetchError::Status(404)) => return Err(SourceError::Unsigned),
            Err(_) => return Ok(None),
        };
        let blob = String::from_utf8(blob).map_err(|_| SourceError::BadSignature)?;
        let signer = verify_signature(&body, blob.trim(), &self.trusted)?;
        let text = std::str::from_utf8(&body).map_err(|_| SourceError::Damaged)?;
        let catalog = Catalog::parse(text)?;
        // Only verified bytes reach the cache. A failed cache write is not an error: the
        // catalogue is still good, the next offline start falls back further.
        let _ = write_atomic(&self.cache, &sig_name, blob.as_bytes())
            .and_then(|()| write_atomic(&self.cache, CATALOG_FILE, &body));
        self.remember(&catalog);
        Ok(Some(LoadedCatalog {
            catalog,
            verified: signer.is_some(),
            signer,
            dir: Some(self.cache.display().to_string()),
        }))
    }
}

impl CatalogSource for OnlineCatalogSource {
    fn name(&self) -> String {
        self.base
            .trim_start_matches("https://")
            .split('/')
            .next()
            .unwrap_or_default()
            .to_string()
    }

    fn load(&self) -> Result<LoadedCatalog, SourceError> {
        if let Some(loaded) = self.load_remote()? {
            return Ok(loaded);
        }
        match FileCatalogSource::new(&self.cache, &self.trusted).load() {
            Ok(loaded) => {
                self.remember(&loaded.catalog);
                Ok(loaded)
            }
            Err(_) => self.fallback.load(),
        }
    }

    fn ensure_package(&self, path: &Path) -> Result<(), SourceError> {
        if path.parent() != Some(self.cache.as_path()) {
            return self.fallback.ensure_package(path);
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(SourceError::Unreadable)?;
        let (sha, size) = {
            let last = self.last.lock().map_err(|_| SourceError::Unreadable)?;
            let entry = last
                .as_ref()
                .and_then(|c| {
                    c.apps
                        .iter()
                        .find(|a| a.pin.package.as_deref() == Some(name))
                })
                .ok_or(SourceError::Unreadable)?;
            let sha = entry
                .pin
                .sha256
                .clone()
                .ok_or(SourceError::PackageMismatch)?;
            (sha, entry.size_download)
        };
        if file_sha256(path).is_some_and(|have| have == sha) {
            return Ok(());
        }
        self.download(name, &sha, size)
    }
}

impl OnlineCatalogSource {
    fn download(&self, name: &str, sha: &str, size: Option<u64>) -> Result<(), SourceError> {
        std::fs::create_dir_all(&self.cache).map_err(|_| SourceError::Unreadable)?;
        let part = self.cache.join(format!("{name}.part"));
        let result = (|| {
            let file = std::fs::File::create(&part).map_err(|_| SourceError::Unreadable)?;
            let mut sink = HashingWriter::new(file);
            let max = size.unwrap_or(MAX_PACKAGE_BYTES).min(MAX_PACKAGE_BYTES);
            match self
                .fetch
                .get(&format!("{}/{name}", self.base), max, &mut sink)
            {
                Ok(n) if size.is_none_or(|s| s == n) => {}
                Ok(_) | Err(FetchError::TooLarge) => return Err(SourceError::PackageMismatch),
                Err(_) => return Err(SourceError::Unreadable),
            }
            let (file, digest) = sink.finish();
            if digest != sha {
                return Err(SourceError::PackageMismatch);
            }
            file.sync_all().map_err(|_| SourceError::Unreadable)?;
            std::fs::rename(&part, self.cache.join(name)).map_err(|_| SourceError::Unreadable)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        result
    }
}

/// A file writer that hashes what passes through it.
struct HashingWriter {
    file: std::fs::File,
    hash: Sha256,
}

impl HashingWriter {
    fn new(file: std::fs::File) -> Self {
        Self {
            file,
            hash: Sha256::new(),
        }
    }

    fn finish(self) -> (std::fs::File, String) {
        (self.file, hex::encode(self.hash.finalize()))
    }
}

impl Write for HashingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.file.write(buf)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// SHA-256 of a file, `None` when it cannot be read.
fn file_sha256(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash).ok()?;
    Some(hex::encode(hash.finalize()))
}

/// Writes `dir/name` atomically (temporary file, fsync, rename).
fn write_atomic(dir: &Path, name: &str, data: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{name}.tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, dir.join(name))
}

#[cfg(feature = "online")]
pub use crate::online_https::UreqFetch;

#[cfg(test)]
#[path = "online_tests.rs"]
mod tests;
