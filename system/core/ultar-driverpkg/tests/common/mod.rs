//! Shared helpers for the integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use ultar_driverpkg::index::{DriverIndex, IndexEntry};
use ultar_driverpkg::manifest::Manifest;
use ultar_driverpkg::package::{self, Limits};
use ultar_driverpkg::payload::build_payload_from;
use ultar_driverpkg::{PrivateKey, TrustedKeys};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A temporary directory removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("udrv-test-{}-{tag}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A minimal valid manifest with the given id, version, rank and `[match]` body.
pub fn manifest_text(id: &str, version: &str, rank: i32, rules: &str) -> String {
    format!(
        "id = \"{id}\"\nversion = \"{version}\"\nclass = \"net\"\nkernel_abi = [\"ultar-kern-abi-1\"]\n\
         origin = \"workshop\"\nrank = {rank}\nname_key = \"driver.{id}.name\"\n\n[match]\n{rules}\n\n\
         [content]\nmodule = \"drivers/{id}.uko\"\n"
    )
}

/// A payload holding a dummy file for every path the manifest names.
pub fn payload_for(manifest: &str) -> Vec<u8> {
    let parsed = Manifest::parse(manifest).unwrap();
    let files: Vec<(String, Vec<u8>)> = parsed
        .content
        .paths()
        .map(|p| (p.to_string(), format!("dummy {p}").into_bytes()))
        .collect();
    build_payload_from(&files).unwrap()
}

/// Packs a manifest with a matching dummy payload.
pub fn pack(manifest: &str, key: &PrivateKey) -> Vec<u8> {
    package::pack(
        manifest.as_bytes(),
        &payload_for(manifest),
        key,
        &Limits::default(),
    )
    .unwrap()
}

/// Builds an in-memory index from manifest texts (file name `<id>-<version>.utr`).
pub fn index_of(manifests: &[String], key: &PrivateKey) -> DriverIndex {
    let trusted = TrustedKeys::from(key.public());
    let entries = manifests
        .iter()
        .map(|text| {
            let parsed = Manifest::parse(text).unwrap();
            let file = format!("{}-{}.utr", parsed.id, parsed.version);
            IndexEntry::from_package(&file, &pack(text, key), &trusted, &Limits::default()).unwrap()
        })
        .collect();
    DriverIndex::new(entries).unwrap()
}

/// The example manifests shipped with the crate, sorted by file name.
pub fn example_manifests() -> Vec<String> {
    let dir = crate_dir().join("../../../drivers/manifests");
    // Built-in drivers of today plus the manifests of drivers still to be written.
    let mut paths: Vec<PathBuf> = [dir.clone(), dir.join("planned")]
        .iter()
        .flat_map(|d| std::fs::read_dir(d).unwrap())
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect()
}
