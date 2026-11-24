//! `ultar-store` – the Ultar Store app (DESKTOP-SPEC §7.3, boards Store, SurfaceStates,
//! CompactStore, CompactStoreApp).
//!
//! The store discovers, searches, installs, updates and removes components from a **local
//! signed catalogue** (offline first; a USB stick or folder can be a catalogue source).
//! Nothing in here contacts the network, and the layers rules of `tools/check-layers.toml`
//! hold: the app uses only `platform/ultar-ui`, `system/core/ultar-bus` (client),
//! `system/core/ultar-i18n` and `system/core/ultar-brand`.
//!
//! | Module | Job |
//! |---|---|
//! | [`catalog`] | the catalogue data: apps, permissions in plain words, categories, file types |
//! | [`online`] | the store server as a catalogue source (signed, cached, offline first) |
//! | [`source`] | [`source::CatalogSource`]: where a catalogue comes from (file, embedded, tests) |
//! | [`search`] | tolerant search over name, description and file type (Arabic insensitive) |
//! | [`state`], [`action`], [`update`] | the UI-independent model: every page, every state, every click |
//! | [`backend`] | the one place that talks to the rest of the system (bus `launcher.*`, files) |
//! | [`view`] | the screens: desktop and compact, built with `ultar-ui` widgets only |
//! | [`i18n`] | the catalogs (Arabic first) |

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod action;
pub mod backend;
pub mod catalog;
pub mod error;
pub mod i18n;
pub mod online;
#[cfg(feature = "online")]
pub mod online_https;
pub mod scripted;
pub mod search;
pub mod signature;
pub mod source;
pub mod state;
pub mod update;
pub mod view;

/// Re-export so the binary and tests import one type; this app uses no other
/// event type.
pub use ultar_ui::event;

/// The catalogue that ships with the system image (read-only; a stick or an
/// update replaces it via `ULTAR_STORE_CATALOG`).
pub const BUILT_IN_CATALOG: &str = include_str!("../assets/ultar-catalog.toml");

pub use state::Store;
