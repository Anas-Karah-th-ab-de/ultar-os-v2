//! ultar-cloud — client of the Ultar cloud (area: system services).
//!
//! **Without an account:** store catalogue (search, categories, app details), installation of
//! free apps, system updates of the open channels. **With an account:** premium apps and gated
//! update channels (if the account's access includes them), the Ultar number, VPN provisioning.
//! The session token is attached only when one is present and only to the configured server.
//!
//! Installation always passes the `ultar_pkg` signature check against the keys built into the
//! device; the server is never the trust anchor.
//!
//! * [`Client`] — explicit server address and token (used by tests and the store service);
//! * the free functions ([`store_list`], [`update_check`], [`register`] …) — the same on the
//!   device's stored address and token;
//! * [`CloudError`] — typed errors with message keys; texts are in `locales/` (ar, en, de).
//!
//! HTTPS is done in Rust (ureq + rustls); TLS is terminated by the reverse proxy of the server.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![deny(missing_docs)]

pub mod client;
pub mod device;
pub mod error;
pub mod http;
pub mod messages;
pub mod model;

pub use client::{Client, VpnPeer};
pub use device::{
    base_url, client, device_id, login, my_number, register, register_number, store_install,
    store_list, token, update_check, vpn_provision, CONF, DEFAULT_BASE, NUMBER_FILE, TOKEN_FILE,
};
pub use error::{CloudError, Subject};
pub use model::{
    AppAccess, AppDetails, AppMeta, AppSummary, CatalogPage, CatalogQuery, Category, Entitlements,
    Publisher, Release, VersionInfo,
};
