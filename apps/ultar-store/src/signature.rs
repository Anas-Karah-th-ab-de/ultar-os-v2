//! Catalogue signatures: Ed25519 over a domain-separated digest, checked against the
//! device's trusted keys (`ultar-catalog-v1`).
//!
//! The implementation lives in the shared crate `ultar-catalog` (`system/core/ultar-catalog`,
//! module `sign`), which the shell uses too; this module re-exports it under the Store's own
//! name. See that module for the blob layout, the key-file trust rules and revocation.

pub use ultar_catalog::sign::*;
