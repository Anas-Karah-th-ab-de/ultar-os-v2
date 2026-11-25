//! ultar-cloud — client of the Ultar cloud (area: system services).
//!
//! **Without an account:** store catalogue (search, categories, app details), installation of
//! free apps, system updates of the open channels. **With an account:** premium apps and gated
//! update channels (if the account's access includes them), the Ultar number, VPN provisioning.
//! **Device identity:** every device has its own Ed25519 key ([`KeyStore`]); the device id is
//! derived from the public key, registration and every device session prove possession of the
//! key to the server (`docs/DEVICE-IDENTITY.md`).
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
pub mod client_device;
pub mod device;
pub mod error;
pub mod http;
pub mod identity;
pub mod keystore;
pub mod messages;
pub mod model;
pub mod signalling;
pub mod watch;

pub use client::{Client, TurnCredentials, VpnPeer};
pub use client_device::{
    authorize_replacement, AccountDevices, AddedDevice, Challenge, DeviceEntry, DeviceEvent,
    RecoveryState, Registration, ReplaceGrant, ReplaceRequest,
};
pub use device::{
    base_url, client, device_id, legacy_device_id, login, my_number, register, register_number,
    store_install, store_list, token, update_check, vpn_provision, DeviceHome, DeviceIdentity,
    CONF, DEFAULT_BASE, NUMBER_FILE, TOKEN_FILE,
};
pub use error::{CloudError, Subject};
pub use keystore::{FileKeyStore, KeyStore, KeyStoreError};
pub use model::{
    AppAccess, AppDetails, AppMeta, AppSummary, CatalogPage, CatalogQuery, Category, Entitlements,
    Publisher, Release, VersionInfo,
};
pub use signalling::{signal_auth, signal_hello, SignalProof};
pub use watch::{SecurityState, SecurityWatch};
