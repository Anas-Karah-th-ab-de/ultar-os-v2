use super::*;
use crate::source::{sign_catalog, EmbeddedCatalogSource};
use ed25519_dalek::SigningKey;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

/// A server double: name → body; `down` simulates no connection.
#[derive(Default)]
struct FakeServer {
    files: Mutex<HashMap<String, Vec<u8>>>,
    down: AtomicBool,
}

struct FakeFetch(Arc<FakeServer>);

impl Fetch for FakeFetch {
    fn get(&self, url: &str, max: u64, sink: &mut dyn Write) -> Result<u64, FetchError> {
        if self.0.down.load(Ordering::SeqCst) {
            return Err(FetchError::Unreachable);
        }
        let name = url.rsplit('/').next().unwrap_or_default();
        let files = self.0.files.lock().unwrap();
        let body = files.get(name).ok_or(FetchError::Status(404))?;
        if body.len() as u64 > max {
            return Err(FetchError::TooLarge);
        }
        sink.write_all(body).map_err(|_| FetchError::Io)?;
        Ok(body.len() as u64)
    }
}

struct Setup {
    _dir: tempfile::TempDir,
    server: Arc<FakeServer>,
    source: OnlineCatalogSource,
    cache: PathBuf,
    key: SigningKey,
}

fn catalog_body(pkg_sha: &str, size: usize) -> String {
    format!(
        "format_version = 1\n\n[[app]]\nid = \"km-code-craft.appbuilder\"\ncategory = \"developer\"\nicon = \"code\"\npublisher = \"KM Code Craft\"\nsource = \"ultar\"\nversion = \"0.8.0\"\nsize_download = {size}\npackage = \"appbuilder-0.8.0.uap\"\nsha256 = \"{pkg_sha}\"\n[app.name]\nar = \"باني التطبيقات\"\nen = \"App Builder\"\n[app.summary]\nar = \"s\"\nen = \"s\"\n[app.description]\nar = \"d\"\nen = \"d\"\n"
    )
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let keys = dir.path().join("keys");
    std::fs::create_dir_all(&keys).unwrap();
    let key = SigningKey::from_bytes(&[4u8; 32]);
    std::fs::write(
        keys.join("km-code-craft.ed25519"),
        hex::encode(key.verifying_key().to_bytes()) + "\n",
    )
    .unwrap();
    let server = Arc::new(FakeServer::default());
    let pkg = b"signed package bytes".to_vec();
    let body = catalog_body(&hex::encode(Sha256::digest(&pkg)), pkg.len());
    publish(&server, &key, &body);
    server
        .files
        .lock()
        .unwrap()
        .insert("appbuilder-0.8.0.uap".into(), pkg);
    let cache = dir.path().join("cache");
    let fallback: Arc<dyn CatalogSource> =
        Arc::new(EmbeddedCatalogSource::new("image", crate::BUILT_IN_CATALOG));
    let source = OnlineCatalogSource::new(
        "https://store.example/store/v1/",
        &cache,
        &keys,
        Box::new(FakeFetch(Arc::clone(&server))),
        fallback,
    )
    .unwrap();
    Setup {
        _dir: dir,
        server,
        source,
        cache,
        key,
    }
}

fn publish(server: &FakeServer, key: &SigningKey, body: &str) {
    let mut files = server.files.lock().unwrap();
    files.insert(CATALOG_FILE.into(), body.as_bytes().to_vec());
    files.insert(
        format!("{CATALOG_FILE}.sig"),
        sign_catalog(body.as_bytes(), key).into_bytes(),
    );
}

#[test]
fn loads_verifies_caches_and_downloads_a_pinned_package() {
    let s = setup();
    assert_eq!(s.source.name(), "store.example");
    let loaded = s.source.load().unwrap();
    assert!(loaded.verified);
    assert_eq!(loaded.catalog.apps.len(), 1);
    assert_eq!(
        loaded.dir.as_deref(),
        Some(s.cache.display().to_string().as_str())
    );
    assert!(s.cache.join(CATALOG_FILE).is_file());
    let pkg = s.cache.join("appbuilder-0.8.0.uap");
    s.source.ensure_package(&pkg).unwrap();
    assert_eq!(std::fs::read(&pkg).unwrap(), b"signed package bytes");
    // Present and matching: no second download (server down does not matter).
    s.server.down.store(true, Ordering::SeqCst);
    s.source.ensure_package(&pkg).unwrap();
}

#[test]
fn offline_uses_the_verified_cache_then_the_image_catalogue() {
    let s = setup();
    s.source.load().unwrap();
    s.server.down.store(true, Ordering::SeqCst);
    let cached = s.source.load().unwrap();
    assert_eq!(cached.catalog.apps[0].id, "km-code-craft.appbuilder");
    // A cache damaged on disk is verified again and refused → image catalogue.
    std::fs::write(s.cache.join(CATALOG_FILE), "format_version = 1\n").unwrap();
    let image = s.source.load().unwrap();
    assert_eq!(image.catalog.apps.len(), 12);
    assert_eq!(image.dir, None);
}

#[test]
fn tampered_or_unsigned_server_catalogue_is_refused_and_not_cached() {
    let s = setup();
    let other = SigningKey::from_bytes(&[5u8; 32]);
    publish(&s.server, &other, &catalog_body(&"0".repeat(64), 1));
    assert_eq!(s.source.load(), Err(SourceError::UnknownSigner));
    assert!(!s.cache.join(CATALOG_FILE).exists());
    // Body changed after signing.
    publish(&s.server, &s.key, &catalog_body(&"0".repeat(64), 1));
    s.server
        .files
        .lock()
        .unwrap()
        .insert(CATALOG_FILE.into(), b"format_version = 1\n".to_vec());
    assert_eq!(s.source.load(), Err(SourceError::UnknownSigner));
    // No signature on the server.
    s.server
        .files
        .lock()
        .unwrap()
        .remove(&format!("{CATALOG_FILE}.sig"));
    assert_eq!(s.source.load(), Err(SourceError::Unsigned));
}

#[test]
fn package_must_match_the_pinned_digest_and_size() {
    let s = setup();
    s.source.load().unwrap();
    s.server.files.lock().unwrap().insert(
        "appbuilder-0.8.0.uap".into(),
        b"evil package bytes!!".to_vec(),
    );
    let pkg = s.cache.join("appbuilder-0.8.0.uap");
    assert_eq!(
        s.source.ensure_package(&pkg),
        Err(SourceError::PackageMismatch)
    );
    assert!(!pkg.exists());
    assert!(!s.cache.join("appbuilder-0.8.0.uap.part").exists());
    // Larger than the signed size.
    s.server
        .files
        .lock()
        .unwrap()
        .insert("appbuilder-0.8.0.uap".into(), vec![0u8; 4096]);
    assert_eq!(
        s.source.ensure_package(&pkg),
        Err(SourceError::PackageMismatch)
    );
    // A package that is not in the verified catalogue is never fetched.
    assert_eq!(
        s.source.ensure_package(&s.cache.join("other.uap")),
        Err(SourceError::Unreadable)
    );
    // Offline download: unreachable.
    s.server.down.store(true, Ordering::SeqCst);
    assert_eq!(s.source.ensure_package(&pkg), Err(SourceError::Unreadable));
}

#[test]
fn unpinned_online_package_is_refused() {
    let s = setup();
    let body =
        catalog_body(&"0".repeat(64), 1).replace(&format!("sha256 = \"{}\"\n", "0".repeat(64)), "");
    publish(&s.server, &s.key, &body);
    s.source.load().unwrap();
    assert_eq!(
        s.source
            .ensure_package(&s.cache.join("appbuilder-0.8.0.uap")),
        Err(SourceError::PackageMismatch)
    );
}

#[test]
fn only_https_base_urls() {
    assert!(valid_base_url(DEFAULT_URL));
    for bad in ["http://x", "ftp://x", "https://a b", "https://x\n", ""] {
        assert!(!valid_base_url(bad), "{bad}");
    }
    let fallback: Arc<dyn CatalogSource> =
        Arc::new(EmbeddedCatalogSource::new("image", crate::BUILT_IN_CATALOG));
    let server = Arc::new(FakeServer::default());
    assert!(OnlineCatalogSource::new(
        "http://plain.example",
        "/tmp/x",
        "/tmp/y",
        Box::new(FakeFetch(server)),
        fallback
    )
    .is_err());
}

#[test]
fn fuzzed_server_answers_never_panic_and_never_verify() {
    let s = setup();
    let mut x: u64 = 0x1234_5678_9abc_def0;
    for round in 0..300 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let len = (x % 300) as usize;
        let junk: Vec<u8> = (0..len)
            .map(|i| (x.rotate_left(i as u32 % 64) & 0xff) as u8)
            .collect();
        {
            let mut files = s.server.files.lock().unwrap();
            if round % 2 == 0 {
                files.insert(CATALOG_FILE.into(), junk);
            } else {
                files.insert(format!("{CATALOG_FILE}.sig"), junk);
            }
        }
        if let Ok(loaded) = s.source.load() {
            // Only the genuine catalogue or a verified fallback can load.
            assert!(loaded.verified);
        }
    }
}
