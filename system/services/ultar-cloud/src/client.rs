//! The cloud client: catalogue, app installation and system updates work signed-out; the token
//! is attached only when one is present. Account-bound calls report [`CloudError::NotSignedIn`]
//! or the server's `auth_required`.
//!
//! Trust: the server only transports packages. [`Client::install_app`] always goes through
//! `ultar_pkg` signature verification against the device's trusted keys; a compromised or
//! impersonated server cannot make the device install anything unsigned.

use crate::error::CloudError;
use crate::http::{Expected, Transport};
use crate::model::{
    AppDetails, AppMeta, CatalogPage, CatalogQuery, Category, Entitlements, Release,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::path::Path;

/// WireGuard peer settings returned by the server.
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct VpnPeer {
    /// Public key of the server.
    pub server_pubkey: String,
    /// Endpoint (`host:port`).
    pub endpoint: String,
    /// Internal address assigned to this device.
    pub address: String,
    /// DNS server inside the tunnel.
    pub dns: String,
    /// Routed networks.
    pub allowed_ips: String,
}

/// Largest app package the client downloads, whatever size the server announces.
pub const MAX_APP_BYTES: u64 = 1024 * 1024 * 1024;

/// Compares two version strings component by component (`.`, `-`, `+` separate components; numeric
/// components compare as numbers, others as text).
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |v: &str| -> Vec<String> { v.split(['.', '-', '+']).map(str::to_string).collect() };
    let (a, b) = (split(a), split(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).map_or("0", String::as_str),
            b.get(i).map_or("0", String::as_str),
        );
        let order = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => x.cmp(y),
        };
        if order != std::cmp::Ordering::Equal {
            return order;
        }
    }
    std::cmp::Ordering::Equal
}

/// Version of the app recorded at its last installation under `root`, if any.
fn installed_version(root: &Path, id: &str) -> Option<String> {
    let text =
        std::fs::read_to_string(root.join("data/ultar/installed").join(format!("{id}.toml")))
            .ok()?;
    text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "version").then(|| value.trim().trim_matches('"').to_string())
    })
}

/// A client for one server.
#[derive(Clone)]
pub struct Client {
    http: Transport,
}

fn parse<T: DeserializeOwned>(value: Value) -> Result<T, CloudError> {
    serde_json::from_value(value).map_err(|_| CloudError::InvalidResponse)
}

fn text_field(value: &Value, field: &str) -> Result<String, CloudError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or(CloudError::InvalidResponse)
}

impl Client {
    /// A client for `base_url`; `token` is `None` when nobody is signed in.
    pub fn new(base_url: &str, token: Option<String>) -> Self {
        Self {
            http: Transport::new(base_url, token),
        }
    }

    /// True when a session token is present.
    pub fn is_signed_in(&self) -> bool {
        self.http.has_token()
    }

    /// The server address.
    pub fn base_url(&self) -> &str {
        self.http.base_url()
    }

    fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, CloudError> {
        parse(
            self.http
                .get_json(path, query)?
                .ok_or(CloudError::InvalidResponse)?,
        )
    }

    // ----- store (no account needed) -----

    /// One page of the catalogue.
    pub fn catalog(&self, query: &CatalogQuery) -> Result<CatalogPage, CloudError> {
        let (page, per_page) = (query.page.to_string(), query.per_page.to_string());
        let mut params = vec![("page", page.as_str()), ("per_page", per_page.as_str())];
        for (key, value) in [
            ("q", &query.text),
            ("category", &query.category),
            ("filetype", &query.file_type),
        ] {
            if !value.is_empty() {
                params.push((key, value.as_str()));
            }
        }
        self.get("/v1/store/catalog", &params)
    }

    /// All categories with their app counts.
    pub fn categories(&self) -> Result<Vec<Category>, CloudError> {
        let value: Value = self.get("/v1/store/categories", &[])?;
        parse(
            value
                .get("categories")
                .cloned()
                .ok_or(CloudError::InvalidResponse)?,
        )
    }

    /// Details of one app (permissions, sizes, version history, publisher, premium flag).
    pub fn app(&self, id: &str) -> Result<AppDetails, CloudError> {
        if !ultar_pkg::valid_id(id) {
            return Err(CloudError::NotFound);
        }
        self.get(&format!("/v1/store/apps/{id}"), &[])
    }

    /// The unpaged listing (shape of the first server release).
    pub fn store_list(&self) -> Result<Vec<AppMeta>, CloudError> {
        self.get("/v1/store/apps", &[])
    }

    /// Downloads the current package of `app` to `dest` and checks size and SHA-256 against the
    /// catalogue. This is a transport check only; the signature is checked at installation.
    pub fn download_app(&self, app: &AppDetails, dest: &Path) -> Result<(), CloudError> {
        // The announced size comes from the server; it must not decide how much disk a download may take.
        if app.summary.size == 0 || app.summary.size > MAX_APP_BYTES {
            return Err(CloudError::Integrity);
        }
        let expected = Expected {
            size: app.summary.size,
            sha256: &app.sha256,
        };
        self.http.download(&app.download, expected, dest)
    }

    /// Downloads and installs an app on the running system.
    pub fn install_app(
        &self,
        id: &str,
        work_dir: &Path,
        trusted_dir: &Path,
    ) -> Result<ultar_pkg::Manifest, CloudError> {
        self.install_app_at(id, work_dir, trusted_dir, Path::new("/"))
    }

    /// Like [`Client::install_app`] with `root` in place of `/`.
    ///
    /// Order: details → download (size, SHA-256) → signature verification against
    /// `trusted_dir` → the signed manifest must name the requested app → installation. If any
    /// step fails nothing is installed and the downloaded file is removed.
    pub fn install_app_at(
        &self,
        id: &str,
        work_dir: &Path,
        trusted_dir: &Path,
        root: &Path,
    ) -> Result<ultar_pkg::Manifest, CloudError> {
        let details = self.app(id)?;
        std::fs::create_dir_all(work_dir)?;
        let file = work_dir.join(format!("{id}.uap"));
        self.download_app(&details, &file)?;
        let outcome = ultar_pkg::verify(&file, trusted_dir)
            .map_err(CloudError::Package)
            .and_then(|manifest| {
                if manifest.id == id {
                    // the server may only move an installed app forward (an older, genuinely signed
                    // package would bring back fixed weaknesses)
                    match installed_version(root, id) {
                        Some(have)
                            if version_cmp(&manifest.version, &have)
                                == std::cmp::Ordering::Less =>
                        {
                            Err(CloudError::Package(format!(
                                "refusing downgrade of {id}: installed {have}, offered {}",
                                manifest.version
                            )))
                        }
                        _ => Ok(()),
                    }
                } else {
                    Err(CloudError::Package(format!(
                        "package is {}, expected {id}",
                        manifest.id
                    )))
                }
            })
            .and_then(|()| {
                ultar_pkg::install_at(&file, trusted_dir, root).map_err(CloudError::Package)
            });
        let _ = std::fs::remove_file(&file);
        outcome
    }

    // ----- system updates (open channels need no account) -----

    /// The newest release of `channel` for this device, `None` when there is none.
    pub fn latest_update(
        &self,
        channel: &str,
        arch: &str,
        edition: &str,
    ) -> Result<Option<Release>, CloudError> {
        let query = [("channel", channel), ("arch", arch), ("edition", edition)];
        self.http
            .get_json("/v1/update/latest", &query)?
            .map(parse)
            .transpose()
    }

    /// Downloads the image of `release` to `dest` and checks size and SHA-256 when announced.
    /// The updater must still verify `roothash` and `sig` against the device's keys before use.
    pub fn download_update(&self, release: &Release, dest: &Path) -> Result<(), CloudError> {
        let expected = Expected {
            size: release.size,
            sha256: &release.sha256,
        };
        self.http.download(&release.url, expected, dest)
    }

    // ----- account -----

    fn credentials(
        &self,
        path: &str,
        username: &str,
        password: &str,
    ) -> Result<String, CloudError> {
        let body = json!({ "username": username, "password": password });
        text_field(&self.http.post_json(path, &body)?, "token")
    }

    /// Creates an account; returns the session token.
    pub fn register(&self, username: &str, password: &str) -> Result<String, CloudError> {
        self.credentials("/v1/account/register", username, password)
    }

    /// Signs in; returns the session token.
    pub fn login(&self, username: &str, password: &str) -> Result<String, CloudError> {
        self.credentials("/v1/account/login", username, password)
    }

    fn require_token(&self) -> Result<(), CloudError> {
        if self.is_signed_in() {
            Ok(())
        } else {
            Err(CloudError::NotSignedIn)
        }
    }

    /// What the signed-in account has access to (premium apps, gated channels).
    pub fn entitlements(&self) -> Result<Entitlements, CloudError> {
        self.require_token()?;
        self.get("/v1/account/entitlements", &[])
    }

    /// Registers the device and returns its Ultar number.
    pub fn register_device(&self, device_id: &str, pubkey: &str) -> Result<String, CloudError> {
        self.require_token()?;
        let body = json!({ "device_id": device_id, "pubkey": pubkey });
        text_field(
            &self.http.post_json("/v1/device/register", &body)?,
            "number",
        )
    }

    /// Provisions a WireGuard peer for the given public key.
    pub fn vpn_peer(&self, pubkey: &str) -> Result<VpnPeer, CloudError> {
        self.require_token()?;
        parse(
            self.http
                .post_json("/v1/vpn/peer", &json!({ "pubkey": pubkey }))?,
        )
    }
}
