//! The Store source of the overview search (`search.source.store`): apps from the Store's
//! catalogue that are **not installed**, offered so choosing one opens the Store on that app
//! (`store://app/<id>`). The shell reads the Store's cached, signature-checked catalogue file
//! (the shared crate `ultar-catalog`, [`FileCatalogSource`]) — never the network. An absent,
//! unsigned or tampered cache offers nothing (fail closed); the switch in Settings gates what
//! is offered regardless.
//!
//! The catalogue data and parser are shared with the Store (`system/core/ultar-catalog`); the
//! platform shell may not depend on the apps area (`tools/check-layers.toml`).

use std::path::{Path, PathBuf};
use ultar_catalog::{store_cache_dir_from_env, trusted_dir_from_env, Catalog, FileCatalogSource};

/// Reads the Store's cached catalogue from `dir`, verified against `trusted`. `None` when it
/// cannot be read or the signature does not verify against a trusted key — the shell offers
/// no Store results then, rather than trusting an unchecked file.
pub fn load(dir: &Path, trusted: &Path) -> Option<Catalog> {
    let loaded = FileCatalogSource::new(dir, trusted).load().ok()?;
    loaded.verified.then_some(loaded.catalog)
}

/// The cached catalogue from the shared cache directory ([`store_cache_dir_from_env`]),
/// verified against the device's trusted keys. Read at start and when the source is switched
/// on; `None` when there is no verified cache.
pub fn load_from_env() -> Option<Catalog> {
    load(&store_cache_dir_from_env(), &trusted_dir_from_env())
}

/// The shared cache directory (re-exported for the app layer and tests).
pub fn cache_dir() -> PathBuf {
    store_cache_dir_from_env()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use ultar_catalog::sign::sign_catalog;
    use ultar_catalog::source::{CATALOG_FILE, CATALOG_SIG_SUFFIX};

    const BODY: &str = "format_version = 1\n\n[[app]]\nid = \"km-office\"\ncategory = \"office\"\nicon = \"office\"\npublisher = \"p\"\nsource = \"ultar\"\nversion = \"1.0.0\"\n[app.name]\nar = \"مكتب\"\nen = \"Office\"\n[app.summary]\nar = \"s\"\nen = \"s\"\n[app.description]\nar = \"d\"\nen = \"d\"\n";

    fn dir(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "ultar-desk-store-{}-{}-{}",
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

    fn keys(signing: &SigningKey) -> PathBuf {
        let k = dir("keys");
        std::fs::create_dir_all(&k).unwrap();
        std::fs::write(
            k.join("v.ed25519"),
            hex::encode(signing.verifying_key().to_bytes()) + "\n",
        )
        .unwrap();
        k
    }

    #[test]
    fn a_verified_cache_is_read_and_an_unchecked_one_is_not() {
        let signing = SigningKey::from_bytes(&[21u8; 32]);
        let cache = dir("cache");
        let trusted = keys(&signing);
        // No cache yet: nothing offered.
        assert!(load(&cache, &trusted).is_none());
        let blob = sign_catalog(BODY.as_bytes(), &signing);
        FileCatalogSource::write_signed(&cache, BODY, &blob).unwrap();
        let catalog = load(&cache, &trusted).unwrap();
        assert_eq!(catalog.apps.len(), 1);
        assert_eq!(catalog.apps[0].id, "km-office");
        // Tampered body: the signature no longer matches → nothing.
        std::fs::write(cache.join(CATALOG_FILE), format!("{BODY}# x\n")).unwrap();
        assert!(load(&cache, &trusted).is_none());
        // Unsigned (signature removed): nothing.
        FileCatalogSource::write_signed(&cache, BODY, &blob).unwrap();
        std::fs::remove_file(cache.join(format!("{CATALOG_FILE}{CATALOG_SIG_SUFFIX}"))).unwrap();
        assert!(load(&cache, &trusted).is_none());
        // A signature by an untrusted key: nothing.
        let other = SigningKey::from_bytes(&[22u8; 32]);
        let blob = sign_catalog(BODY.as_bytes(), &other);
        FileCatalogSource::write_signed(&cache, BODY, &blob).unwrap();
        assert!(load(&cache, &trusted).is_none());
        std::fs::remove_dir_all(&cache).unwrap();
        std::fs::remove_dir_all(&trusted).unwrap();
    }
}
