//! The catalogue data (DESKTOP-SPEC §7.2/§7.3): components from Ultar and their
//! permissions in plain words.
//!
//! The data model and its parser live in the shared crate `ultar-catalog`
//! (`system/core/ultar-catalog`), which the shell's overview search uses too — one
//! implementation, no duplication (the platform shell may not depend on the apps area,
//! `tools/check-layers.toml`). This module re-exports it under the Store's own name.

pub use ultar_catalog::model::*;
