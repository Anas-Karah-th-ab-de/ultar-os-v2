//! Radio switches (`rfkill`): which radios exist and whether they are blocked; soft
//! blocks are changed through `dev/rfkill` (one 8-byte `struct rfkill_event` per
//! write, `RFKILL_OP_CHANGE_ALL` per radio type). A hard block (a switch on the case)
//! cannot be lifted by software and is reported as it is.

use crate::sysfs::{entries, number, text};
use std::io::Write;
use std::path::PathBuf;
use ultar_hal::{HalError, HalResult};

/// Kinds of radio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RadioKind {
    /// Wi-Fi.
    Wlan,
    /// Bluetooth.
    Bluetooth,
    /// Mobile network.
    Wwan,
}

impl RadioKind {
    fn number(self) -> u8 {
        match self {
            RadioKind::Wlan => 1,
            RadioKind::Bluetooth => 2,
            RadioKind::Wwan => 5,
        }
    }

    fn from_word(word: &str) -> Option<Self> {
        match word {
            "wlan" => Some(RadioKind::Wlan),
            "bluetooth" => Some(RadioKind::Bluetooth),
            "wwan" => Some(RadioKind::Wwan),
            _ => None,
        }
    }
}

/// One radio switch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Radio {
    /// Kind.
    pub kind: RadioKind,
    /// Blocked by software.
    pub soft: bool,
    /// Blocked by a hardware switch.
    pub hard: bool,
}

/// The radio switches below a root directory.
#[derive(Clone, Debug)]
pub struct Rfkill {
    root: PathBuf,
}

/// `RFKILL_OP_CHANGE_ALL`.
const OP_CHANGE_ALL: u8 = 3;

/// The 8-byte event that soft-blocks (`block`) every radio of `kind`.
pub fn change_all(kind: RadioKind, block: bool) -> [u8; 8] {
    [0, 0, 0, 0, kind.number(), OP_CHANGE_ALL, u8::from(block), 0]
}

impl Rfkill {
    /// Switches below `root` (`/` on a device).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Every radio switch the kernel lists.
    pub fn radios(&self) -> Vec<Radio> {
        entries(&self.root.join("sys/class/rfkill"), 64)
            .into_iter()
            .filter_map(|p| {
                Some(Radio {
                    kind: RadioKind::from_word(&text(&p.join("type"))?)?,
                    soft: number(&p.join("soft")) == Some(1),
                    hard: number(&p.join("hard")) == Some(1),
                })
            })
            .collect()
    }

    /// Whether radios of `kind` exist, and whether all of them are blocked.
    pub fn state(&self, kind: RadioKind) -> Option<bool> {
        let of: Vec<Radio> = self
            .radios()
            .into_iter()
            .filter(|r| r.kind == kind)
            .collect();
        (!of.is_empty()).then(|| of.iter().all(|r| r.soft || r.hard))
    }

    /// Soft-blocks or unblocks every radio of `kind`.
    pub fn set(&self, kind: RadioKind, block: bool) -> HalResult<()> {
        // Append: a no-op on the character device, keeps every event in a test file.
        let mut dev = std::fs::OpenOptions::new()
            .append(true)
            .open(self.root.join("dev/rfkill"))
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => HalError::NotSupported,
                std::io::ErrorKind::PermissionDenied => HalError::PermissionDenied,
                _ => HalError::Io(e.raw_os_error().unwrap_or(0)),
            })?;
        dev.write_all(&change_all(kind, block))
            .map_err(|e| HalError::Io(e.raw_os_error().unwrap_or(0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &std::path::Path, path: &str, value: &str) {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, value).unwrap();
    }

    #[test]
    fn radios_are_read_and_changes_written() {
        let root = std::env::temp_dir().join(format!("hal-rfkill-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        put(&root, "sys/class/rfkill/rfkill0/type", "wlan\n");
        put(&root, "sys/class/rfkill/rfkill0/soft", "0\n");
        put(&root, "sys/class/rfkill/rfkill0/hard", "0\n");
        put(&root, "sys/class/rfkill/rfkill1/type", "bluetooth\n");
        put(&root, "sys/class/rfkill/rfkill1/soft", "1\n");
        put(&root, "sys/class/rfkill/rfkill2/type", "nfc\n");
        let r = Rfkill::new(&root);
        assert_eq!(r.radios().len(), 2);
        assert_eq!(r.state(RadioKind::Wlan), Some(false));
        assert_eq!(r.state(RadioKind::Bluetooth), Some(true));
        assert_eq!(r.state(RadioKind::Wwan), None);
        assert_eq!(
            r.set(RadioKind::Wlan, true),
            Err(HalError::NotSupported),
            "no control device"
        );
        put(&root, "dev/rfkill", "");
        r.set(RadioKind::Bluetooth, false).unwrap();
        assert_eq!(
            std::fs::read(root.join("dev/rfkill")).unwrap(),
            [0, 0, 0, 0, 2, 3, 0, 0]
        );
        assert_eq!(change_all(RadioKind::Wwan, true), [0, 0, 0, 0, 5, 3, 1, 0]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
