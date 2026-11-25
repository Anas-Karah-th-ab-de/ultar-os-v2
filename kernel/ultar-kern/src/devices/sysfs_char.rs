//! `/sys/dev/char/<major>:<minor>`: the way from a device node to the device behind it, as Linux
//! lays it out. Graphics libraries (libdrm) find the PCI device of a render node here: they read
//! the link `device/subsystem` and the files `device/uevent`, `device/vendor`, `device/device`.
//! Only the render node of the Intel graphics has an entry.

use crate::driver::DEVICES;
use crate::fs::{DirEnt, Node};
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Device number of the render node (`svcdev`: 226:128).
const RENDER: &str = "226:128";
const PCI_CLASS_DISPLAY: u8 = 0x03;
const VENDOR_INTEL: u16 = 0x8086;

/// PCI name of the graphics device behind the render node; `None` while the node does not exist.
fn render_device() -> Option<String> {
    if !crate::svcdev::dri::present() {
        return None;
    }
    DEVICES.lock().iter().find(|b| b.dev.vendor == VENDOR_INTEL && b.dev.class == PCI_CLASS_DISPLAY).map(|b| super::pci_name(&b.dev))
}

fn dir(names: &[&str], dirs: bool) -> Option<Node> {
    Some(Node::Dir(names.iter().map(|n| DirEnt { name: String::from(*n), dir: dirs, size: 0 }).collect()))
}

/// Node at `/sys/dev/<rest>`.
pub fn open(rest: &[&str]) -> Option<Node> {
    match rest {
        [] => dir(&["char"], true),
        ["char"] => match render_device() {
            Some(_) => dir(&[RENDER], true),
            None => dir(&[], true),
        },
        ["char", id, tail @ ..] if *id == RENDER => {
            let pci = render_device()?;
            match tail {
                [] => Some(Node::Dir(alloc::vec![
                    DirEnt { name: String::from("device"), dir: true, size: 0 },
                    DirEnt { name: String::from("uevent"), dir: false, size: 0 },
                ])),
                ["uevent"] => {
                    let text = format!("MAJOR=226\nMINOR=128\nDEVNAME={}\nDEVTYPE=drm_minor\n", crate::svcdev::dri::NODE);
                    Some(Node::File(Arc::new(text.into_bytes())))
                }
                ["device", below @ ..] => super::sysfs::pci_device(&pci, below),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Target of a symbolic link at `/sys/dev/<rest>`.
pub fn link(rest: &str) -> Option<String> {
    let p: Vec<&str> = rest.split('/').collect();
    match p.as_slice() {
        ["char", id, "device", "subsystem"] if *id == RENDER && render_device().is_some() => Some(String::from("../../../bus/pci")),
        ["char", id, "subsystem"] if *id == RENDER && render_device().is_some() => Some(String::from("../../../class/drm")),
        _ => None,
    }
}
