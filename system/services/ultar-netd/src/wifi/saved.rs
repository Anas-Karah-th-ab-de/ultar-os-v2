//! Saved Wi-Fi networks: the name and the derived key (PMK, 32 bytes – the
//! password itself is never stored), most recently used first, in a checked
//! state file of mode 0600 (`ultar_svckit::store`). Never logged.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ultar_netproto::wpa::Pmk;
use ultar_svckit::store::{self, StoreError};
use zeroize::Zeroize;

/// Most networks kept.
pub const MAX_SAVED: usize = 64;

#[derive(Serialize, Deserialize, Default)]
struct Stored {
    networks: Vec<(Vec<u8>, Option<[u8; 32]>)>,
}

impl Drop for Stored {
    fn drop(&mut self) {
        for (_, key) in &mut self.networks {
            if let Some(k) = key {
                k.zeroize();
            }
        }
    }
}

/// One saved network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// SSID bytes.
    pub ssid: Vec<u8>,
    /// Key of a secured network (`None`: open).
    pub pmk: Option<Pmk>,
}

/// The saved networks.
#[derive(Debug)]
pub struct Saved {
    path: PathBuf,
    list: Vec<Entry>,
}

impl Saved {
    /// Loads `path` (missing or damaged: none saved).
    pub fn load(path: PathBuf) -> Saved {
        let list = store::load::<Stored>(&path)
            .map(|s| {
                s.networks
                    .iter()
                    .take(MAX_SAVED)
                    .map(|(ssid, key)| Entry {
                        ssid: ssid.clone(),
                        pmk: key.map(Pmk::from_bytes),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Saved { path, list }
    }

    /// The entry of `ssid`.
    pub fn get(&self, ssid: &[u8]) -> Option<&Entry> {
        self.list.iter().find(|e| e.ssid == ssid)
    }

    /// Nothing saved.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Names, most recently used first.
    pub fn names(&self) -> Vec<String> {
        self.list
            .iter()
            .map(|e| String::from_utf8_lossy(&e.ssid).into_owned())
            .collect()
    }

    /// Position of `ssid` (0: most recently used).
    pub fn rank(&self, ssid: &[u8]) -> Option<usize> {
        self.list.iter().position(|e| e.ssid == ssid)
    }

    fn save(&self) -> Result<(), StoreError> {
        let stored = Stored {
            networks: self
                .list
                .iter()
                .map(|e| (e.ssid.clone(), e.pmk.as_ref().map(|p| *p.expose())))
                .collect(),
        };
        store::save(&self.path, &stored)
    }

    /// Saves (or moves to the front) a network.
    pub fn remember(&mut self, ssid: &[u8], pmk: Option<Pmk>) -> Result<(), StoreError> {
        self.list.retain(|e| e.ssid != ssid);
        self.list.insert(
            0,
            Entry {
                ssid: ssid.to_vec(),
                pmk,
            },
        );
        self.list.truncate(MAX_SAVED);
        self.save()
    }

    /// Removes a network; `false` when it was not saved.
    pub fn forget(&mut self, ssid: &[u8]) -> Result<bool, StoreError> {
        let before = self.list.len();
        self.list.retain(|e| e.ssid != ssid);
        if self.list.len() == before {
            return Ok(false);
        }
        self.save().map(|()| true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn kept_0600_most_recent_first_without_the_password() {
        let dir = std::env::temp_dir().join(format!("netd-saved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wifi");
        let mut s = Saved::load(path.clone());
        assert!(s.is_empty());
        let pmk = Pmk::from_passphrase("very secret words", b"home").unwrap();
        s.remember(b"home", Some(pmk.clone())).unwrap();
        s.remember(b"cafe", None).unwrap();
        s.remember(b"home", Some(pmk.clone())).unwrap();
        assert_eq!(s.names(), vec!["home", "cafe"]);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.windows(17).any(|w| w == b"very secret words"));
        let again = Saved::load(path.clone());
        assert_eq!(again.get(b"home").unwrap().pmk, Some(pmk));
        assert_eq!(again.rank(b"cafe"), Some(1));
        let mut again = again;
        assert!(again.forget(b"cafe").unwrap());
        assert!(!again.forget(b"cafe").unwrap());
        for i in 0..70u8 {
            again.remember(&[b'n', i], None).unwrap();
        }
        assert_eq!(Saved::load(path.clone()).names().len(), MAX_SAVED);
        std::fs::write(&path, b"damaged").unwrap();
        assert!(Saved::load(path).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
