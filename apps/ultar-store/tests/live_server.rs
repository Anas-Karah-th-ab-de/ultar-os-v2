//! Smoke test against the real store server (ignored by default; needs the network):
//!   ULTAR_LIVE_STORE_KEY=<hex store public key> \
//!   cargo test -p ultar-store --features online --test live_server -- --ignored
//! Loads the published catalogue through the device code path (HTTPS, signature check against
//! the given key only, cache) and downloads every package against its pinned digest.
#![cfg(feature = "online")]

use std::sync::Arc;
use ultar_store::online::{OnlineCatalogSource, UreqFetch, DEFAULT_URL};
use ultar_store::source::{CatalogSource, EmbeddedCatalogSource};

#[test]
#[ignore = "needs the network and ULTAR_LIVE_STORE_KEY"]
fn published_catalogue_and_packages_verify() {
    let key = std::env::var("ULTAR_LIVE_STORE_KEY").expect("ULTAR_LIVE_STORE_KEY");
    let dir = tempfile::tempdir().unwrap();
    let keys = dir.path().join("keys");
    std::fs::create_dir_all(&keys).unwrap();
    std::fs::write(
        keys.join("km-code-craft.ed25519"),
        format!("{}\n", key.trim()),
    )
    .unwrap();
    let cache = dir.path().join("cache");
    let image: Arc<dyn CatalogSource> = Arc::new(EmbeddedCatalogSource::new(
        "image",
        ultar_store::BUILT_IN_CATALOG,
    ));
    let url = std::env::var("ULTAR_STORE_URL").unwrap_or_else(|_| DEFAULT_URL.to_string());
    let fetch = UreqFetch::new().expect("TLS provider on this CPU");
    let source = OnlineCatalogSource::new(&url, &cache, &keys, Box::new(fetch), image).unwrap();
    let loaded = source.load().unwrap();
    assert!(loaded.verified);
    assert!(
        loaded.signer.is_some(),
        "came from the server, not the image"
    );
    assert_eq!(
        loaded.dir.as_deref(),
        Some(cache.display().to_string().as_str())
    );
    assert!(!loaded.catalog.apps.is_empty());
    for app in &loaded.catalog.apps {
        let file = app.pin.package.as_deref().expect("package");
        source.ensure_package(&cache.join(file)).unwrap();
        println!(
            "ok {} {} {}",
            app.id,
            file,
            app.pin.sha256.as_deref().unwrap_or("-")
        );
    }
}
