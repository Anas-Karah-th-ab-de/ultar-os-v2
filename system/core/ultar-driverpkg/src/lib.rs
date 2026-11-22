//! Signed Ultar driver packages (`.utr`), hardware matching and the driver depot.
//!
//! **Area:** system core (plan §3.0, area 3) – tooling and logic for area 2 (drivers).
//! This crate implements plan §5.1–§5.3:
//!
//! * [`package`] – the `.utr` container: an uncompressed tar holding
//!   `ultar-driver.toml` (manifest), `payload.tar.gz` (module, firmware, HAL
//!   configuration) and `sig.ed25519`. The signature covers the canonical message
//!   `"ultar-driver-v1\n<sha256(manifest)>\n<sha256(payload)>\n"`. A package that is
//!   unsigned, signed by an unknown key or modified is rejected with a distinct error.
//! * [`manifest`] – strictly validated driver metadata with typed hardware patterns.
//! * [`hardware`] / [`matching`] – device descriptions and the candidate engine
//!   (exact id > wildcard > class, then rank, then version, deterministic).
//! * [`index`] / [`depot`] – the signed `index.toml` of a depot directory, a fast
//!   lookup structure, [`depot::resolve`] and [`depot::select_for_install`].
//! * [`cli`] / [`msg`] – the `udrv` tool; all output goes through message keys
//!   (Arabic first, then English and German).
//!
//! Cryptography is not implemented here: Ed25519 comes from `ed25519-dalek`,
//! SHA-256 from `sha2` – the same approach as the application package format.
//!
//! No hardware is accessed; device lists are supplied by the caller (HAL, installer).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![deny(missing_docs)]

pub mod class;
pub mod cli;
pub mod depot;
pub mod error;
pub mod hardware;
pub mod hwlist;
pub mod index;
pub mod keys;
pub mod manifest;
pub mod matching;
pub mod msg;
pub mod package;
pub mod pattern;
pub mod payload;
pub mod sign;
pub mod version;

pub use class::{DriverClass, Origin};
pub use depot::{
    resolve, select_for_install, BlueprintRequest, DevicePlan, InstallSelection, Plan,
};
pub use error::{Coded, Error};
pub use hardware::{DeviceDescriptor, HardwareId};
pub use index::{DriverIndex, IndexEntry};
pub use keys::{PrivateKey, PublicKey, TrustedKeys};
pub use manifest::{Content, Manifest, MatchRules};
pub use matching::{candidates, Candidate, MatchKind, Specificity};
pub use package::{Limits, VerifiedPackage};
pub use version::Version;
