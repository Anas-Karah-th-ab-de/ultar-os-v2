//! Files of the Ultar account link, in the service's own folder (`<dir>/link`, `0700`):
//!
//! * `link.json` – server, user name, number, services of this device (no secret);
//! * `account.token` – the account token (`0600`), never leaves this service;
//! * the relay token (`--relay-token`, `0640` in a folder `0750`, both of the group
//!   `--relay-group` that only this service and the mesh service share): the device token the
//!   mesh service signs in at the Verbund relay with. Removed on sign-out.
//!
//! Every file is written whole (temporary file, then rename). A file that cannot be read
//! counts as absent: the device is then signed out, never half signed in.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use ultar_bus::messages::account_service::AccountService;

/// Longest token read back.
const MAX_TOKEN: u64 = 4096;

/// What the link keeps between starts (no secret).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    /// Server host; empty = the default.
    #[serde(default)]
    pub server: String,
    /// User name of the signed-in account.
    #[serde(default)]
    pub user: String,
    /// The Ultar number of this device.
    #[serde(default)]
    pub number: String,
    /// Services switched on for this device.
    #[serde(default)]
    pub services: Vec<AccountService>,
}

/// The link's files.
#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
    relay_token: Option<PathBuf>,
    relay_group: Option<u32>,
}

fn write_whole(path: &Path, bytes: &[u8], mode: u32) -> bool {
    let tmp = path.with_extension("new");
    let written = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(&tmp)
        .and_then(|mut f| {
            f.write_all(bytes)?;
            f.sync_all()
        })
        .and_then(|()| fs::set_permissions(&tmp, fs::Permissions::from_mode(mode)))
        .and_then(|()| fs::rename(&tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written.is_ok()
}

fn read_small(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_TOKEN {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

impl Store {
    /// The files under `<service dir>/link`; `relay_token` is where the mesh service reads the
    /// relay token (none: no relay), `relay_group` the group it reads it as.
    pub fn new(service_dir: &Path, relay_token: Option<PathBuf>, relay_group: Option<u32>) -> Self {
        let dir = service_dir.join("link");
        let _ = fs::create_dir_all(&dir);
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
        Self {
            dir,
            relay_token,
            relay_group,
        }
    }

    /// The saved state (default when absent or unreadable).
    pub fn load(&self) -> Saved {
        fs::read(self.dir.join("link.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Saves the state; false when it could not be written.
    pub fn save(&self, saved: &Saved) -> bool {
        serde_json::to_vec(saved).is_ok_and(|b| write_whole(&self.dir.join("link.json"), &b, 0o600))
    }

    /// The account token, if signed in.
    pub fn token(&self) -> Option<String> {
        read_small(&self.dir.join("account.token"))
    }

    /// Keeps the account token; false when it could not be written.
    pub fn save_token(&self, token: &str) -> bool {
        write_whole(&self.dir.join("account.token"), token.as_bytes(), 0o600)
    }

    /// Forgets the account token.
    pub fn clear_token(&self) {
        let _ = fs::remove_file(self.dir.join("account.token"));
    }

    /// Hands the device token to the mesh service (its relay sign-in).
    pub fn write_relay_token(&self, token: &str) {
        let Some(path) = &self.relay_token else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
            let _ = std::os::unix::fs::chown(parent, None, self.relay_group);
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o750));
        }
        if write_whole(path, token.as_bytes(), 0o640) {
            let _ = std::os::unix::fs::chown(path, None, self.relay_group);
        }
    }

    /// Takes the relay token away (signed out).
    pub fn clear_relay_token(&self) {
        if let Some(path) = &self.relay_token {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_and_tokens_round_trip_privately() {
        let d = tempfile::tempdir().unwrap();
        let relay = d.path().join("relay/relay.token");
        let s = Store::new(d.path(), Some(relay.clone()), None);
        assert_eq!(s.load(), Saved::default());
        assert_eq!(s.token(), None);
        let saved = Saved {
            server: "x.example".into(),
            user: "anas".into(),
            number: "UOS-AAAA-0001".into(),
            services: vec![AccountService::Calls],
        };
        assert!(s.save(&saved) && s.save_token("t1"));
        s.write_relay_token("d1");
        assert_eq!((s.load(), s.token()), (saved, Some("t1".into())));
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&d.path().join("link/account.token")), 0o600);
        assert_eq!(mode(&d.path().join("link")), 0o700);
        assert_eq!(
            (mode(&relay), fs::read_to_string(&relay).unwrap()),
            (0o640, "d1".into())
        );
        s.clear_token();
        s.clear_relay_token();
        assert_eq!(s.token(), None);
        assert!(!relay.exists());
        fs::write(d.path().join("link/link.json"), b"{broken").unwrap();
        assert_eq!(s.load(), Saved::default());
    }
}
