//! ultraos-server — the Ultar cloud for `ultar.code-craft.tech` (area: server).
//!
//! Services:
//! * **Public, no account:** store catalogue (paging, search, categories, app details), download
//!   of free packages, newest system release of the open channels and its image.
//! * **Account + entitlement:** premium packages and gated update channels.
//! * **Account:** TURN credentials, WireGuard provisioning, Ultar-Verbund (device list and
//!   end-to-end relay), password change.
//! * **Device key:** registration with proof of possession and the unique Ultar number, device
//!   tokens, device list / rename / remove / replace, call signalling (WebSocket) – see
//!   `docs/DEVICE-IDENTITY.md`.
//!
//! The server is never a trust anchor: packages and system images are signed by the publisher
//! and verified on the device. The server only transports them and decides who may fetch what.
//!
//! TLS is terminated in front of this process (Caddy). See `DEPLOY.md` and `API.md`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![deny(missing_docs)]

pub mod accounts;
pub mod admin;
pub mod auth;
pub mod catalog;
pub mod challenge;
mod challenge_purge;
pub mod challenge_session;
pub mod cli;
pub mod clock;
pub mod conditional;
pub mod conntable;
pub mod db;
pub mod device_api;
pub mod device_auth;
pub mod device_error;
pub mod device_events;
pub mod device_model;
pub mod device_moves;
pub mod device_moves_api;
pub mod devices;
pub mod devtoken;
pub mod entitlements;
pub mod error;
pub mod files;
pub mod guard;
pub mod identity;
pub mod migrate;
mod migrate_v2;
pub mod msg;
pub mod pwguard;
pub mod ratelimit;
pub mod search;
pub mod server;
pub mod signal;
pub mod store_api;
pub mod updates;
pub mod verbund;

use axum::extract::{ws::WebSocketUpgrade, DefaultBodyLimit, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use clock::Clock;
use db::{Db, DbError};
use files::FileStore;
use guard::ProxyNet;
use ratelimit::{LimiterConfig, RateLimiter};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;

/// Everything the server needs to start.
#[derive(Debug, Clone)]
pub struct Config {
    /// Path of the SQLite database file.
    pub db_path: String,
    /// Directory of the content-addressed file store (packages, system images).
    pub files_dir: PathBuf,
    /// 32-byte key for session tokens.
    pub secret: [u8; 32],
    /// Public WireGuard key of the server.
    pub wg_server_pubkey: String,
    /// WireGuard endpoint (`host:port`).
    pub wg_endpoint: String,
    /// coturn `static-auth-secret` (empty = TURN not configured).
    pub turn_secret: String,
    /// Public TURN/STUN host.
    pub turn_host: String,
    /// Reverse proxies whose `X-Forwarded-For` is believed.
    pub trusted_proxies: Vec<ProxyNet>,
    /// Rate limits.
    pub limits: LimiterConfig,
    /// Device identity: server name, device limit, recovery cool-down.
    pub devices: device_auth::DeviceConfig,
    /// Connections open at the same time.
    pub max_connections: usize,
    /// Connections of one client (trusted proxies exempt).
    pub max_connections_per_peer: usize,
}

impl Config {
    /// A configuration with the given storage locations and token key; everything else default
    /// (loopback is the only trusted proxy, TURN unconfigured).
    pub fn new(
        db_path: impl Into<String>,
        files_dir: impl Into<PathBuf>,
        secret: [u8; 32],
    ) -> Self {
        Self {
            db_path: db_path.into(),
            files_dir: files_dir.into(),
            secret,
            wg_server_pubkey: String::new(),
            wg_endpoint: "ultar.code-craft.tech:51820".to_string(),
            turn_secret: String::new(),
            turn_host: "ultar.code-craft.tech".to_string(),
            trusted_proxies: vec![
                ProxyNet::host(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                ProxyNet::host(IpAddr::V6(Ipv6Addr::LOCALHOST)),
            ],
            limits: LimiterConfig::default(),
            devices: device_auth::DeviceConfig::default(),
            max_connections: server::DEFAULT_MAX_CONNECTIONS,
            max_connections_per_peer: server::DEFAULT_MAX_CONNECTIONS_PER_PEER,
        }
    }
}

/// Shared state of all request handlers.
pub struct AppState {
    /// The database.
    pub db: Db,
    /// Key for session tokens.
    pub secret: [u8; 32],
    /// Call signalling: number → socket.
    pub hub: signal::Hub,
    /// Ultar-Verbund: relay between devices of the same account.
    pub verbund: verbund::Hub,
    /// Counter for internal WireGuard addresses (10.80.x.y).
    pub wg_next: AtomicU32,
    /// Public WireGuard key of the server.
    pub wg_server_pubkey: String,
    /// WireGuard endpoint.
    pub wg_endpoint: String,
    /// coturn use-auth-secret (its own secret, not the token key).
    pub turn_secret: String,
    /// Public TURN/STUN host (not behind the CDN proxy).
    pub turn_host: String,
    /// File store for packages and system images.
    pub files: FileStore,
    /// Per-client rate limiter.
    pub limiter: RateLimiter,
    /// Reverse proxies whose `X-Forwarded-For` is believed.
    pub trusted_proxies: Vec<ProxyNet>,
    /// Device authentication: challenges, limits, time.
    pub devices: device_auth::DeviceAuth,
    /// Connections open at the same time.
    pub max_connections: usize,
    /// Connections of one client (trusted proxies exempt).
    pub max_connections_per_peer: usize,
}

impl AppState {
    /// Opens (and migrates) the database and builds the shared state.
    pub fn new(config: Config, clock: Arc<dyn Clock>) -> Result<Arc<Self>, DbError> {
        let db = Db::open(&config.db_path)?;
        let wg_start = accounts::next_wg_host(&db.lock())?;
        Ok(Arc::new(Self {
            db,
            secret: config.secret,
            hub: signal::Hub::default(),
            verbund: verbund::Hub::default(),
            wg_next: AtomicU32::new(wg_start),
            wg_server_pubkey: config.wg_server_pubkey,
            wg_endpoint: config.wg_endpoint,
            turn_secret: config.turn_secret,
            turn_host: config.turn_host,
            files: FileStore::new(config.files_dir),
            limiter: RateLimiter::new(config.limits, clock.clone()),
            trusted_proxies: config.trusted_proxies,
            devices: device_auth::DeviceAuth::new(config.devices, clock),
            max_connections: config.max_connections,
            max_connections_per_peer: config.max_connections_per_peer,
        }))
    }
}

async fn health() -> &'static str {
    "ok"
}

async fn signal_ws(State(st): State<Arc<AppState>>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.max_message_size(signal::MAX_MESSAGE_BYTES)
        .max_frame_size(signal::MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| signal::handle(socket, st))
}

/// Builds the router with all routes, the body limit and the request guard.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/account/register", post(accounts::account_register))
        .route("/v1/account/login", post(accounts::account_login))
        .route(
            "/v1/account/entitlements",
            get(accounts::account_entitlements),
        )
        .route("/v1/account/password", post(accounts::account_password))
        .route("/v1/device/challenge", post(device_api::challenge))
        .route("/v1/device/register", post(device_api::register))
        .route("/v1/device/session", post(device_api::session))
        .route("/v1/device/upgrade", post(device_api::upgrade))
        .route("/v1/device/rename", post(device_api::rename))
        .route("/v1/device/remove", post(device_api::remove))
        .route("/v1/device/replace", post(device_moves_api::replace))
        .route("/v1/device/approve", post(device_moves_api::approve))
        .route(
            "/v1/device/recover/start",
            post(device_moves_api::recover_start),
        )
        .route(
            "/v1/device/recover/cancel",
            post(device_moves_api::recover_cancel),
        )
        .route(
            "/v1/device/recover/finish",
            post(device_moves_api::recover_finish),
        )
        .route("/v1/device/events", get(device_api::events))
        .route("/v1/devices", get(device_api::list))
        .route("/v1/signal", get(signal_ws))
        .route("/v1/turn-cred", get(accounts::turn_cred))
        .route("/v1/vpn/peer", post(accounts::vpn_peer))
        .route("/v1/store/apps", get(store_api::store_apps))
        .route("/v1/store/catalog", get(store_api::catalog_list))
        .route("/v1/store/categories", get(store_api::categories))
        .route("/v1/store/apps/:id", get(store_api::app_details))
        .route("/v1/store/apps/:id/download", get(store_api::app_download))
        .route("/v1/update/latest", get(updates::update_latest))
        .route("/v1/update/file/:id", get(updates::update_file))
        .route("/v1/verbund/geraet", post(verbund::geraet))
        .route("/v1/verbund/geraet/entfernen", post(verbund::entfernen))
        .route("/v1/verbund/geraete", get(verbund::geraete))
        .route("/v1/verbund/relais", get(verbund::relais_ws))
        .route("/v1/verbund/statistik", get(verbund::statistik))
        .fallback(|| async { error::ApiError::NotFound })
        .layer(DefaultBodyLimit::max(guard::MAX_BODY_BYTES))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            guard::guard,
        ))
        .with_state(state)
}

pub use server::serve;
