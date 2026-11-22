//! Device-bound state: server address, session token and Ultar number in their fixed places
//! under `/data/ultar`, and the operations that read or write them.

use crate::client::Client;
use crate::error::CloudError;
use crate::http::write_atomic;
use crate::model::{AppMeta, Release};
use std::path::Path;

/// File with the server address in its first line.
pub const CONF: &str = "/data/ultar/cloud.conf";
/// File with the session token (owner-readable only).
pub const TOKEN_FILE: &str = "/data/ultar/cloud.token";
/// File with the device's Ultar number.
pub const NUMBER_FILE: &str = "/data/ultar/number";
/// Server used when no address is configured.
pub const DEFAULT_BASE: &str = "https://ultar.code-craft.tech";

const DEVICE_PUBKEY_FILE: &str = "/data/ultar/device.pub";
const DOWNLOAD_DIR: &str = "/data/ultar/dl";
const WG_PEER_FILE: &str = "/run/ultar/wg-peer.conf";

fn first_line(path: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let line = text.lines().next().unwrap_or("").trim().to_string();
    (!line.is_empty()).then_some(line)
}

/// The configured server address, or [`DEFAULT_BASE`].
pub fn base_url() -> String {
    first_line(CONF).unwrap_or_else(|| DEFAULT_BASE.to_string())
}

/// The stored session token, if someone is signed in.
pub fn token() -> Option<String> {
    first_line(TOKEN_FILE)
}

/// The stored Ultar number, if the device is registered.
pub fn my_number() -> Option<String> {
    first_line(NUMBER_FILE)
}

/// A client for this device: configured server, token attached only when present.
pub fn client() -> Client {
    Client::new(&base_url(), token())
}

fn save_token(token: &str) -> Result<(), CloudError> {
    Ok(write_atomic(Path::new(TOKEN_FILE), token.as_bytes(), true)?)
}

/// Creates an account and stores the session token.
pub fn register(username: &str, password: &str) -> Result<String, CloudError> {
    let token = client().register(username, password)?;
    save_token(&token)?;
    Ok(token)
}

/// Signs in and stores the session token.
pub fn login(username: &str, password: &str) -> Result<String, CloudError> {
    let token = client().login(username, password)?;
    save_token(&token)?;
    Ok(token)
}

/// 64-bit FNV-1a over serial number and device key: a stable, non-reversible short id
/// (uniqueness is enough here; this is not a cryptographic commitment).
pub fn short_id(serial: &str, pubkey: &str) -> String {
    let mut h: u64 = 1469598103934665603;
    for b in serial.trim().bytes().chain(pubkey.trim().bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(1099511628211);
    }
    format!("{h:016x}")
}

/// Stable device id from the serial number and the device public key.
pub fn device_id() -> String {
    let serial = std::fs::read_to_string("/proc/cmdline")
        .ok()
        .and_then(|c| {
            c.split_whitespace()
                .find_map(|k| k.strip_prefix("androidboot.serialno=").map(str::to_string))
        })
        .or_else(|| std::fs::read_to_string("/sys/class/android_usb/android0/iSerial").ok())
        .unwrap_or_else(|| "unknown".into());
    let pubkey = std::fs::read_to_string(DEVICE_PUBKEY_FILE).unwrap_or_default();
    short_id(&serial, &pubkey)
}

/// Registers this device and stores its Ultar number. Needs an account.
pub fn register_number() -> Result<String, CloudError> {
    let pubkey = std::fs::read_to_string(DEVICE_PUBKEY_FILE).unwrap_or_default();
    let number = client().register_device(&device_id(), pubkey.trim())?;
    write_atomic(Path::new(NUMBER_FILE), number.as_bytes(), false)?;
    Ok(number)
}

/// Generates a WireGuard key pair, registers the public key and stores the peer settings
/// (owner-readable only; the private key never leaves the device). Needs an account.
pub fn vpn_provision() -> Result<(), CloudError> {
    let cloud = client();
    if !cloud.is_signed_in() {
        return Err(CloudError::NotSignedIn);
    }
    let (private_key, public_key) = ultar_vpn::genkeys().map_err(CloudError::Vpn)?;
    let peer = cloud.vpn_peer(&public_key)?;
    let conf = format!(
        "private_key={private_key}\nserver_pubkey={}\nendpoint={}\naddress={}\ndns={}\nallowed_ips={}\n",
        peer.server_pubkey, peer.endpoint, peer.address, peer.dns, peer.allowed_ips
    );
    Ok(write_atomic(
        Path::new(WG_PEER_FILE),
        conf.as_bytes(),
        true,
    )?)
}

/// The store listing. Works signed-out.
pub fn store_list() -> Result<Vec<AppMeta>, CloudError> {
    client().store_list()
}

/// Downloads and installs an app; returns `"<name> v<version>"`. Works signed-out for free apps.
/// The package signature is verified against the device's trusted keys before anything is
/// written.
pub fn store_install(app: &AppMeta) -> Result<String, CloudError> {
    let manifest = client().install_app(
        &app.id,
        Path::new(DOWNLOAD_DIR),
        Path::new(ultar_pkg::TRUSTED_DIR),
    )?;
    Ok(format!("{} v{}", manifest.name, manifest.version))
}

/// The newest release of `channel` for this device's architecture. Works signed-out for open
/// channels.
pub fn update_check(channel: &str) -> Result<Option<Release>, CloudError> {
    client().latest_update(channel, std::env::consts::ARCH, "any")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_id_is_stable_and_input_dependent() {
        let a = short_id("SERIAL1", "pubkey");
        assert_eq!(a, short_id(" SERIAL1\n", "pubkey "));
        assert_eq!(a.len(), 16);
        assert_ne!(a, short_id("SERIAL2", "pubkey"));
        assert_ne!(a, short_id("SERIAL1", "other"));
        assert_eq!(short_id("", ""), "14650fb0739d0383");
    }

    #[test]
    fn first_line_handles_missing_and_blank_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conf");
        assert_eq!(first_line(&path.to_string_lossy()), None);
        std::fs::write(&path, "\n").unwrap();
        assert_eq!(first_line(&path.to_string_lossy()), None);
        std::fs::write(&path, "  https://x.example \nsecond").unwrap();
        assert_eq!(
            first_line(&path.to_string_lossy()).as_deref(),
            Some("https://x.example")
        );
    }
}
