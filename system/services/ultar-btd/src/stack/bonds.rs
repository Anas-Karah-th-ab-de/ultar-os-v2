//! Bonds: the keys of paired devices, kept in one checked state file (`ULTARSTATE1`,
//! CRC, atomic replace, mode 0600 – `ultar_svckit::store`) inside the service's own
//! directory (0700, owner `btd`). Keys never appear in logs, on the bus or in `Debug`.

use super::hci::Addr;
use serde::{Deserialize, Serialize};
use std::path::Path;
use zeroize::Zeroize;

/// Most bonds kept (the oldest is dropped beyond).
pub const MAX_BONDS: usize = 32;

/// What kind of device a bond is.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Keyboard.
    Keyboard,
    /// Mouse / pointing device.
    Mouse,
    /// Keyboard and mouse.
    Combo,
    /// Audio device (not connected in this round).
    Audio,
    /// Phone.
    Phone,
    /// Computer.
    Computer,
    /// Anything else.
    Other,
}

/// One paired device.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Bond {
    /// Address (identity address for LE devices that gave one).
    pub addr: Addr,
    /// LE (else BR/EDR).
    pub le: bool,
    /// LE: LTK (HCI order); BR/EDR: link key.
    pub key: [u8; 16],
    /// BR/EDR link key type (HCI), 0 for LE.
    pub key_type: u8,
    /// Paired with protection against a man in the middle.
    pub authenticated: bool,
    /// LE: the device's IRK (big-endian) to recognise its private addresses.
    pub irk: Option<[u8; 16]>,
    /// Name when paired (data).
    pub name: String,
    /// Kind.
    pub kind: Kind,
}

impl std::fmt::Debug for Bond {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bond")
            .field("addr", &self.addr.text())
            .field("le", &self.le)
            .finish_non_exhaustive()
    }
}

impl Drop for Bond {
    fn drop(&mut self) {
        self.key.zeroize();
        if let Some(irk) = self.irk.as_mut() {
            irk.zeroize();
        }
    }
}

/// The bond table.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Eq, Debug)]
pub struct Bonds {
    /// Oldest first.
    pub list: Vec<Bond>,
}

impl Bonds {
    /// The bond of an address, also through an LE private address its IRK resolves.
    pub fn find(&self, addr: &Addr) -> Option<&Bond> {
        self.list.iter().find(|b| b.addr == *addr).or_else(|| {
            if !addr.random {
                return None;
            }
            self.list.iter().find(|b| {
                b.irk
                    .as_ref()
                    .is_some_and(|irk| super::crypto::resolves(irk, &addr.bytes))
            })
        })
    }

    /// Adds or replaces a bond (the newest last); beyond [`MAX_BONDS`] the oldest goes.
    pub fn put(&mut self, bond: Bond) {
        self.list.retain(|b| b.addr != bond.addr);
        self.list.push(bond);
        while self.list.len() > MAX_BONDS {
            self.list.remove(0);
        }
    }

    /// Removes a bond; whether there was one.
    pub fn remove(&mut self, addr: &Addr) -> bool {
        let before = self.list.len();
        self.list.retain(|b| b.addr != *addr);
        before != self.list.len()
    }

    /// Reads the file; a missing or damaged file is an empty table (never overwritten
    /// silently: the caller saves only after a change).
    pub fn load(path: &Path) -> Self {
        ultar_svckit::store::load(path).unwrap_or_default()
    }

    /// Writes the file (0600, atomic).
    pub fn save(&self, path: &Path) -> bool {
        ultar_svckit::store::save(path, self).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn bond(last: u8) -> Bond {
        Bond {
            addr: Addr::public([last, 1, 2, 3, 4, 5]),
            le: true,
            key: [last; 16],
            key_type: 0,
            authenticated: true,
            irk: None,
            name: "k".into(),
            kind: Kind::Keyboard,
        }
    }

    #[test]
    fn bonds_are_kept_owner_only_and_limited() {
        let dir = std::env::temp_dir().join(format!("btd-bonds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bonds");
        let mut b = Bonds::default();
        for i in 0..40 {
            b.put(bond(i));
        }
        assert_eq!(b.list.len(), MAX_BONDS);
        assert!(
            b.find(&Addr::public([0, 1, 2, 3, 4, 5])).is_none(),
            "oldest dropped"
        );
        assert!(b.save(&path));
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(Bonds::load(&path), b);
        assert!(!format!("{:?}", b).contains("39, 39"), "no keys in Debug");
        let mut bytes = std::fs::read(&path).unwrap();
        let n = bytes.len();
        bytes[n / 2] ^= 1;
        std::fs::write(&path, bytes).unwrap();
        assert!(
            Bonds::load(&path).list.is_empty(),
            "damaged file: nothing trusted"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn private_addresses_resolve_to_their_bond() {
        let mut b = Bonds::default();
        let mut k = bond(9);
        let irk = [
            0x9b, 0x7d, 0x39, 0x0a, 0xa6, 0x10, 0x10, 0x34, 0x05, 0xad, 0xc8, 0x57, 0xa3, 0x34,
            0x02, 0xec,
        ];
        let mut be = irk;
        be.reverse();
        k.irk = Some(be);
        b.put(k);
        let rpa = Addr {
            bytes: [0xaa, 0xfb, 0x0d, 0x94, 0x81, 0x70],
            random: true,
        };
        assert_eq!(
            b.find(&rpa).map(|x| x.addr),
            Some(Addr::public([9, 1, 2, 3, 4, 5]))
        );
        assert!(b.remove(&Addr::public([9, 1, 2, 3, 4, 5])));
        assert!(b.find(&rpa).is_none());
    }
}
