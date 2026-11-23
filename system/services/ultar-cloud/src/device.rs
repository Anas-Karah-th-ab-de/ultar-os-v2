//! Device-bound state: server address, session token, device key and Ultar number in their fixed
//! places under `/data/ultar`, and the operations that read or write them.
//!
//! The device's identity is its key ([`crate::keystore`]); the device id is derived from the
//! public key, not from serial numbers (`docs/DEVICE-IDENTITY.md`). [`DeviceHome`] does the same
//! on any directory, which is what the tests use.

use crate::client::Client;
use crate::client_device::Registration;
use crate::error::CloudError;
use crate::http::write_atomic;
use crate::identity;
use crate::keystore::{FileKeyStore, KeyStore};
use crate::model::{AppMeta, Release};
use std::path::{Path, PathBuf};

/// File with the server address in its first line.
pub const CONF: &str = "/data/ultar/cloud.conf";
/// File with the session token (owner-readable only).
pub const TOKEN_FILE: &str = "/data/ultar/cloud.token";
/// File with the device's Ultar number.
pub const NUMBER_FILE: &str = "/data/ultar/number";
/// Server used when no address is configured.
pub const DEFAULT_BASE: &str = "https://ultar.code-craft.tech";

const DATA_DIR: &str = "/data/ultar";
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

/// 64-bit FNV-1a over serial number and the old key file: the device id of the first release.
/// Not an identity any more – only used to find this device's earlier registration once, so that
/// its number can be kept when the device gets its key ([`DeviceHome::register_number`]).
pub fn short_id(serial: &str, pubkey: &str) -> String {
    let mut h: u64 = 1469598103934665603;
    for b in serial.trim().bytes().chain(pubkey.trim().bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(1099511628211);
    }
    format!("{h:016x}")
}

/// The id this device registered with before device keys existed (serial number + old key file).
pub fn legacy_device_id() -> String {
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

/// What this device knows about its registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    /// Name of the server the device is registered with (as it enters signatures).
    pub server: String,
    /// Account the device belongs to.
    pub account_id: i64,
    /// Key id of the device.
    pub device_id: String,
    /// Its Ultar number.
    pub number: String,
}

impl DeviceIdentity {
    fn to_text(&self) -> String {
        format!(
            "ultar-device-state-v1\nserver={}\naccount_id={}\ndevice_id={}\nnumber={}\n",
            self.server, self.account_id, self.device_id, self.number
        )
    }

    fn from_text(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        if lines.next()? != "ultar-device-state-v1" {
            return None;
        }
        let mut field = |name: &str| -> Option<String> {
            let (key, value) = lines.next()?.split_once('=')?;
            (key == name && !value.is_empty()).then(|| value.to_string())
        };
        Some(Self {
            server: field("server")?,
            account_id: field("account_id")?.parse().ok()?,
            device_id: field("device_id")?,
            number: field("number")?,
        })
    }
}

/// The directory that holds the device's cloud state (`/data/ultar` on a device): device key,
/// registration state, number, account token.
#[derive(Debug, Clone)]
pub struct DeviceHome {
    dir: PathBuf,
}

impl DeviceHome {
    /// The state directory of the running system.
    pub fn system() -> Self {
        Self::new(DATA_DIR)
    }

    /// A state directory at `dir` (tests, other roots).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// The file key store of this device (the key is created at first use).
    pub fn key_store(&self) -> Result<FileKeyStore, CloudError> {
        Ok(FileKeyStore::open_or_create(self.file("device.key"))?)
    }

    /// The key-derived device id (creates the key at first use).
    pub fn device_id(&self) -> Result<String, CloudError> {
        Ok(identity::device_id(&self.key_store()?.public_key()?))
    }

    /// The stored registration, if the device is registered with its key. A damaged state file
    /// counts as "not registered"; the server is the authority and registration is idempotent.
    pub fn identity(&self) -> Option<DeviceIdentity> {
        let text = std::fs::read_to_string(self.file("device.state")).ok()?;
        DeviceIdentity::from_text(&text)
    }

    /// The stored Ultar number.
    pub fn number(&self) -> Option<String> {
        first_line(&self.file("number").to_string_lossy())
    }

    fn save(&self, client: &Client, done: &Registration) -> Result<(), CloudError> {
        let state = DeviceIdentity {
            server: client.server_name()?,
            account_id: done.account_id,
            device_id: done.device_id.clone(),
            number: done.number.clone(),
        };
        write_atomic(&self.file("device.state"), state.to_text().as_bytes(), true)?;
        write_atomic(&self.file("number"), done.number.as_bytes(), false)?;
        Ok(())
    }

    /// Registers this device with its key and stores its number. Needs an account.
    ///
    /// A device that was registered before device keys (it has a number but no key state) first
    /// tries to take that registration over with `legacy_id`, so that it keeps its number; if the
    /// server does not allow that, it registers as a new device.
    pub fn register_number(
        &self,
        client: &Client,
        name: &str,
        legacy_id: Option<&str>,
    ) -> Result<String, CloudError> {
        if !client.is_signed_in() {
            return Err(CloudError::NotSignedIn);
        }
        let key = self.key_store()?;
        let had_legacy_registration = self.identity().is_none() && self.number().is_some();
        let upgraded = match legacy_id.filter(|_| had_legacy_registration) {
            Some(id) => match client.upgrade_legacy_device(&key, id, name) {
                Ok(done) => Some(done),
                Err(CloudError::LegacyNotEligible) => None,
                Err(other) => return Err(other),
            },
            None => None,
        };
        let done = match upgraded {
            Some(done) => done,
            None => client.register_device(&key, name)?,
        };
        self.save(client, &done)?;
        Ok(done.number)
    }

    /// Deletes key, state and number: the device has no identity until it registers again (new
    /// id, new number). For an explicit reset by the user, e.g. after the device was removed
    /// from its account.
    pub fn reset(&self) -> Result<(), CloudError> {
        FileKeyStore::new(self.file("device.key")).destroy()?;
        for name in ["device.state", "number"] {
            match std::fs::remove_file(self.file(name)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}

/// The key-derived id of this device (creates the device key at first use).
pub fn device_id() -> Result<String, CloudError> {
    DeviceHome::system().device_id()
}

/// Registers this device with its key and stores its Ultar number. Needs an account.
pub fn register_number() -> Result<String, CloudError> {
    DeviceHome::system().register_number(&client(), "", Some(&legacy_device_id()))
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
        // The constant id every device without a serial number had (finding S15).
        assert_eq!(short_id("unknown", ""), "7af8c5b4c52e91a3");
    }

    #[test]
    fn identity_state_round_trips_and_rejects_damage() {
        let state = DeviceIdentity {
            server: "127.0.0.1:8080".into(),
            account_id: 7,
            device_id: "a".repeat(32),
            number: "UOS-0001-0002".into(),
        };
        assert_eq!(
            DeviceIdentity::from_text(&state.to_text()),
            Some(state.clone())
        );
        let text = state.to_text();
        for damaged in [
            "",
            "other-header\n",
            &text.replace("account_id=7", "account_id=x"),
            &text.replace("number=UOS-0001-0002", "number="),
            &text.replace("server=", "sever="),
            &text[..text.len() / 2],
        ] {
            assert_eq!(DeviceIdentity::from_text(damaged), None, "{damaged:?}");
        }
    }

    #[test]
    fn a_home_has_a_stable_key_derived_id_and_can_be_reset() {
        let dir = tempfile::tempdir().unwrap();
        let home = DeviceHome::new(dir.path().join("ultar"));
        assert_eq!(home.identity(), None);
        assert_eq!(home.number(), None);
        let id = home.device_id().unwrap();
        assert_eq!(id.len(), 32);
        assert_eq!(home.device_id().unwrap(), id, "same key, same id");
        let other = DeviceHome::new(dir.path().join("other"));
        assert_ne!(other.device_id().unwrap(), id, "no shared constant id");
        home.reset().unwrap();
        home.reset().unwrap();
        assert_ne!(
            home.device_id().unwrap(),
            id,
            "a reset device is a new device"
        );
    }

    #[test]
    fn registering_needs_an_account() {
        let dir = tempfile::tempdir().unwrap();
        let home = DeviceHome::new(dir.path());
        let signed_out = Client::new("http://127.0.0.1:9", None);
        assert!(matches!(
            home.register_number(&signed_out, "", None),
            Err(CloudError::NotSignedIn)
        ));
        assert!(!dir.path().join("device.key").exists(), "nothing created");
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
