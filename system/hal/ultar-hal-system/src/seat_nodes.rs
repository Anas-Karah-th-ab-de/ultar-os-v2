//! Where the seat's device nodes live, for `ultar-devd-seat`: the node files
//! below `/dev` (input event nodes, DRM cards, framebuffers, `uinput`) and the
//! sysfs entry of an input node (`/sys/class/input/<event>`), which tells
//! hardware from software devices (`/devices/virtual/`). Linux and the Ultar
//! kernel use the same layout. Every path is relative to a root (`/`; a
//! prepared tree in tests).

use std::path::{Path, PathBuf};

/// Directories below `/dev` that hold seat nodes, with the prefix of the
/// node names (relative to `/dev`) found in them.
pub const NODE_DIRS: [(&str, &str); 3] =
    [("dev", ""), ("dev/input", "input/"), ("dev/dri", "dri/")];

/// The node directory `/dev` below `root`.
pub fn dev_dir(root: &Path) -> PathBuf {
    root.join("dev")
}

/// The node of `devname` (relative to `/dev`) below `root`.
pub fn node_path(root: &Path, devname: &str) -> PathBuf {
    dev_dir(root).join(devname)
}

/// The sysfs entry of input node `event` (`event3`) below `root`.
pub fn input_class(root: &Path, event: &str) -> PathBuf {
    root.join("sys/class/input").join(event)
}

/// The device name file of an input node's sysfs entry (`input_class`).
pub fn input_name(class: &Path) -> PathBuf {
    class.join("device/name")
}

/// The file that makes the kernel announce input node `event` again (write
/// `change`).
pub fn input_uevent(root: &Path, event: &str) -> PathBuf {
    input_class(root, event).join("uevent")
}

/// True if the resolved sysfs path of a device belongs to a device made by
/// software (`uinput` and the like).
pub fn is_virtual_device(resolved: &Path) -> bool {
    resolved.to_string_lossy().contains("/devices/virtual/")
}

/// `DEVNAME` of a device event, relative to `/dev` (Linux sends it relative;
/// the absolute form is accepted too).
pub fn relative_devname(value: &str) -> &str {
    value.strip_prefix("/dev/").unwrap_or(value)
}

/// The sound card's running streams of the Ultar kernel (48 kHz, 16 bit,
/// stereo; one opener, right `sound`) below `root`.
pub fn pcm_node(root: &Path) -> PathBuf {
    node_path(root, "ultar/pcm")
}

/// The first camera of the Ultar kernel (one MJPEG picture per read; one opener, right
/// `camera`) below `root`.
pub fn camera_node(root: &Path) -> PathBuf {
    node_path(root, "ultar/kamera")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_below_the_root() {
        let root = Path::new("/r");
        assert_eq!(
            node_path(root, "input/event1"),
            PathBuf::from("/r/dev/input/event1")
        );
        assert_eq!(
            input_class(root, "event1"),
            PathBuf::from("/r/sys/class/input/event1")
        );
        assert!(is_virtual_device(Path::new(
            "/sys/devices/virtual/input/input9/event5"
        )));
        assert!(!is_virtual_device(Path::new(
            "/sys/devices/pci0000:00/usb1/input/input3/event2"
        )));
        assert_eq!(relative_devname("/dev/fb0"), "fb0");
        assert_eq!(relative_devname("input/event2"), "input/event2");
    }
}
