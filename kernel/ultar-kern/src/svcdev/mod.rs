//! Device nodes for privileged system services below `/dev/ultar/`:
//!
//! * `hciN` – the raw HCI transport of USB Bluetooth controller N (`hci.rs`), for the Bluetooth
//!   service; right [`Rights::BLUETOOTH`]; one open descriptor per controller;
//! * `uinput` – virtual keyboards, mice and tablets (`uinput.rs`), for the Bluetooth service and
//!   remote control; right [`Rights::INPUT_INJECT`].
//!
//! * `pcm` – the sound card's running streams (`pcm.rs`) for the call service; right
//!   [`Rights::SOUND`]; one open descriptor at a time.
//!
//! * `kamera` – the pictures of the first USB camera (`kamera.rs`) for the call service; right
//!   [`Rights::CAMERA`]; one open descriptor at a time.
//!
//! * `/dev/dri/renderD128` – the GPU for programs (`dri.rs`, Linux `xe` uAPI); for now right
//!   [`Rights::RAW_DEVICE`], and only present once the GT driver published its device record.
//!
//! The gate of these nodes is their right alone (default deny): `RAW_DEVICE`, `FILE_ADMIN`, the
//! node's owner and mode open nothing here. The nodes report root-owned `0600`.

use crate::isolation::rights::Rights;
use alloc::string::String;
use alloc::vec::Vec;

pub mod dri;
pub mod hci;
pub mod kamera;
pub mod pcm;
pub mod uinput;

/// Names below `/dev/` of the controller nodes (index = node number).
const HCI_NODES: [&str; crate::drivers::gen_xhci::bt::MAX_CONTROLLERS] = ["ultar/hci0", "ultar/hci1", "ultar/hci2", "ultar/hci3"];
/// Name below `/dev/` of the virtual input node.
pub const UINPUT_NODE: &str = "ultar/uinput";

/// The node `d` (path below `/dev/`) if it is one of these and exists now.
pub fn node(d: &str) -> Option<&'static str> {
    if d == UINPUT_NODE {
        return Some(UINPUT_NODE);
    }
    if d == pcm::NODE {
        return pcm::present().then_some(pcm::NODE);
    }
    if d == kamera::NODE {
        return kamera::present().then_some(kamera::NODE);
    }
    if d == dri::NODE {
        return dri::present().then_some(dri::NODE);
    }
    let i = HCI_NODES.iter().position(|n| *n == d)?;
    crate::drivers::gen_xhci::bt::nodes().contains(&(i as u8)).then_some(HCI_NODES[i])
}

/// The names in `/dev/ultar` that belong to this module.
pub fn names() -> Vec<String> {
    let mut v: Vec<String> = crate::drivers::gen_xhci::bt::nodes().into_iter().map(|i| alloc::format!("hci{i}")).collect();
    v.push(String::from("uinput"));
    if pcm::present() {
        v.push(String::from("pcm"));
    }
    if kamera::present() {
        v.push(String::from("kamera"));
    }
    v
}

/// The right that opens the node at host path `path`, if it is one of these.
pub fn right_for(path: &str) -> Option<Rights> {
    let d = path.strip_prefix("/dev/")?;
    if d == UINPUT_NODE {
        Some(Rights::INPUT_INJECT)
    } else if d == pcm::NODE {
        Some(Rights::SOUND)
    } else if d == kamera::NODE {
        Some(Rights::CAMERA)
    } else if d == dri::NODE {
        Some(dri::RIGHT)
    } else if HCI_NODES.contains(&d) {
        Some(Rights::BLUETOOTH)
    } else {
        None
    }
}

/// Number of controller node `d` (`ultar/hciN`).
fn hci_index(d: &str) -> Option<u8> {
    HCI_NODES.iter().position(|n| *n == d).map(|i| i as u8)
}

/// Open node `d` (a name from [`node`]) for a caller holding `rights`.
pub fn open(d: &str, rights: Rights) -> Result<crate::proc::FdKind, i64> {
    let need = right_for(&alloc::format!("/dev/{d}")).ok_or(-ENOENT)?;
    if !rights.has(need) {
        return Err(-EACCES);
    }
    if d == dri::NODE {
        return dri::open();
    }
    if d == pcm::NODE {
        return pcm::open();
    }
    if d == kamera::NODE {
        return kamera::open();
    }
    match hci_index(d) {
        Some(i) => hci::open(i),
        None => Ok(uinput::open()),
    }
}

/// `st_rdev` of a node: character major 10 (misc) like Linux, minor 223 for `uinput` and
/// 240 + N for `hciN`.
pub fn rdev(d: &str) -> Option<u64> {
    if d == UINPUT_NODE {
        return Some(10 << 8 | 223);
    }
    if d == dri::NODE {
        // DRM major 226, render nodes start at minor 128
        return Some(226 << 8 | 128);
    }
    if d == pcm::NODE {
        // sound major 116 like Linux ALSA
        return Some(116 << 8 | 16);
    }
    if d == kamera::NODE {
        // video4linux major 81
        return Some(81 << 8);
    }
    hci_index(d).map(|i| 10 << 8 | (240 + i as u64))
}

pub(crate) const ENOENT: i64 = 2;
pub(crate) const ENOTTY: i64 = 25;
pub(crate) const EIO: i64 = 5;
pub(crate) const EAGAIN: i64 = 11;
pub(crate) const EBUSY: i64 = 16;
pub(crate) const ENODEV: i64 = 19;
pub(crate) const EINVAL: i64 = 22;
pub(crate) const ENOSPC: i64 = 28;
pub(crate) const EACCES: i64 = 13;
pub(crate) const EFAULT: i64 = 14;
pub(crate) const ENOMEM: i64 = 12;
