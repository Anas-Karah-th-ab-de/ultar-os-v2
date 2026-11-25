//! The camera light as a torch, ported from the first Ultar OS (tested on the Galaxy A30):
//!
//! * Samsung phones: `sys/class/camera/flash/rear_flash` (`1` on, `0` off). It cannot be
//!   read back, so the state is the last value written.
//! * Others: an LED class device whose name contains `torch` or `flash`
//!   (`sys/class/leds/<name>/brightness`, on = `max_brightness`).
//! * Never the Samsung PMIC LED `sys/class/leds/*s2mu*`: writing it crashes the stock kernel.
//!
//! Every path is relative to a root (`/` on a device, a prepared tree in tests).

use std::path::{Path, PathBuf};

/// The Samsung camera flash control.
pub const SAMSUNG: &str = "sys/class/camera/flash/rear_flash";
/// The LED class directory.
pub const LEDS: &str = "sys/class/leds";
/// LED names never touched (Samsung PMIC: the stock kernel crashes).
const FORBIDDEN: &str = "s2mu";
/// Most LED entries looked at.
const MAX_LEDS: usize = 64;

fn number(path: &Path) -> Option<i64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn entries(dir: &Path, limit: usize) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().take(limit).map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// Where the torch is switched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TorchNode {
    /// The Samsung flash control (write `1`/`0`).
    Samsung(PathBuf),
    /// An LED class device (its directory).
    Led(PathBuf),
}

impl TorchNode {
    /// The file that is written (devd-seat gives it to the power service).
    pub fn control(&self) -> PathBuf {
        match self {
            TorchNode::Samsung(p) => p.clone(),
            TorchNode::Led(dir) => dir.join("brightness"),
        }
    }
}

/// The torch of the system below `root`, if it has one.
pub fn find(root: &Path) -> Option<TorchNode> {
    let samsung = root.join(SAMSUNG);
    if samsung.exists() {
        return Some(TorchNode::Samsung(samsung));
    }
    let mut leds = entries(&root.join(LEDS), MAX_LEDS);
    leds.sort();
    leds.into_iter()
        .find(|dir| {
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            !name.contains(FORBIDDEN) && (name.contains("torch") || name.contains("flash"))
        })
        .map(TorchNode::Led)
}

/// Whether an LED torch is on (`None` for the Samsung control, which cannot be read).
pub fn is_on(node: &TorchNode) -> Option<bool> {
    match node {
        TorchNode::Samsung(_) => None,
        TorchNode::Led(dir) => number(&dir.join("brightness")).map(|b| b > 0),
    }
}

/// Switches the torch.
pub fn set(node: &TorchNode, on: bool) -> std::io::Result<()> {
    match node {
        TorchNode::Samsung(p) => std::fs::write(p, if on { "1" } else { "0" }),
        TorchNode::Led(dir) => {
            let max = number(&dir.join("max_brightness")).unwrap_or(1).max(1);
            let value = if on { max.to_string() } else { "0".into() };
            std::fs::write(dir.join("brightness"), value)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("torch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn put(root: &Path, path: &str, value: &str) {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, value).unwrap();
    }

    #[test]
    fn samsung_first_and_written_as_one_or_zero() {
        let root = tree("samsung");
        put(&root, SAMSUNG, "0");
        put(&root, "sys/class/leds/torch-light0/brightness", "0");
        let node = find(&root).unwrap();
        assert!(matches!(node, TorchNode::Samsung(_)));
        assert_eq!(is_on(&node), None);
        set(&node, true).unwrap();
        assert_eq!(std::fs::read_to_string(root.join(SAMSUNG)).unwrap(), "1");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_led_torch_at_full_brightness_but_never_the_pmic_led() {
        let root = tree("led");
        put(&root, "sys/class/leds/fled-s2mu106/brightness", "0");
        assert_eq!(find(&root), None, "the PMIC LED is never a torch");
        put(&root, "sys/class/leds/white:flash/brightness", "0");
        put(&root, "sys/class/leds/white:flash/max_brightness", "255");
        let node = find(&root).unwrap();
        assert_eq!(
            node.control(),
            root.join("sys/class/leds/white:flash/brightness")
        );
        set(&node, true).unwrap();
        assert_eq!(is_on(&node), Some(true));
        set(&node, false).unwrap();
        assert_eq!(is_on(&node), Some(false));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn no_torch() {
        assert_eq!(find(&tree("none")), None);
    }
}
