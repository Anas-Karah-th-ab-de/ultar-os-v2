//! `ultar-catalog` – the signed Ultar catalogue, shared by the Store (`apps/ultar-store`)
//! and the shell's overview search (`platform/ultar-desktop`).
//!
//! The catalogue data model and its TOML parser ([`model`]), the Ed25519 signature check
//! against the device's trusted keys ([`sign`]) and the file source that reads, verifies and
//! parses a catalogue directory ([`source`]) live here so both sides use **one** implementation
//! (the platform shell may not depend on the apps area, `tools/check-layers.toml`). Nothing in
//! here contacts the network: a catalogue is a local TOML body plus a detached signature.
//!
//! | Module | Job |
//! |---|---|
//! | [`model`] | the catalogue data: apps, permissions in plain words, categories, file types |
//! | [`sign`] | `ultar-catalog-v1` signatures: Ed25519 over `sha256(body)`, trusted keys, revocation |
//! | [`source`] | [`source::FileCatalogSource`]: read a directory, verify, parse (single capped read) |

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod model;
pub mod sign;
pub mod source;

pub use model::{Catalog, CatalogApp, Loc, PackagePin, Permission, Source, FORMAT_VERSION};
pub use source::{
    store_cache_dir_from_env, trusted_dir_from_env, FileCatalogSource, LoadedCatalog, SourceError,
};
