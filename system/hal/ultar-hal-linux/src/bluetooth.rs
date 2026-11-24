//! Bluetooth controllers: the list (`sys/class/bluetooth/hciN`) and the raw HCI **user
//! channel** of one controller (`AF_BLUETOOTH`/`BTPROTO_HCI` bound to
//! `HCI_CHANNEL_USER`): the kernel's own host stack lets go of the controller and every
//! H4 packet goes to and from the one process holding the channel (needs
//! `CAP_NET_ADMIN`; the controller must be down – `EBUSY` otherwise).
//!
//! The Ultar kernel has no Bluetooth socket family (`socket()` fails there). It offers
//! each USB Bluetooth controller as a **raw HCI node** `dev/ultar/hciN` instead
//! ([`RawNode`]): the same H4 packets, one per `read`/`write`; only a process holding
//! the kernel right `bluetooth` opens it, one at a time.
#![allow(unsafe_code)]

use rustix::event::{poll, PollFd, PollFlags, Timespec};
use rustix::net::addr::{SocketAddrArg, SocketAddrLen, SocketAddrOpaque};
use rustix::net::{AddressFamily, Protocol, SocketType};
use std::num::NonZeroU32;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `BTPROTO_HCI`.
const BTPROTO_HCI: u32 = 1;
/// `HCI_CHANNEL_USER`.
const HCI_CHANNEL_USER: u16 = 1;
/// The longest H4 packet (ACL: 4 + 65535 is never used by controllers; events ≤ 258).
pub const MAX_PACKET: usize = 4096;

/// The controllers the kernel lists (`hci0` → 0), sorted.
pub fn controllers(root: &Path) -> Vec<u16> {
    let Ok(dir) = std::fs::read_dir(root.join("sys/class/bluetooth")) else {
        return Vec::new();
    };
    let mut out: Vec<u16> = dir
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_prefix("hci")?.parse().ok())
        .collect();
    out.sort_unstable();
    out
}

/// The raw HCI nodes of the Ultar kernel below `root` (`dev/ultar/hciN`), by number.
pub fn ultar_nodes(root: &Path) -> Vec<PathBuf> {
    let dir = root.join("dev/ultar");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut found: Vec<(u16, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_str()?.strip_prefix("hci")?.parse().ok()?;
            Some((n, e.path()))
        })
        .collect();
    found.sort_unstable();
    found.into_iter().map(|(_, p)| p).collect()
}

/// One H4 packet from `fd`, waiting at most `timeout`; `None` when none came.
fn read_packet(fd: &OwnedFd, timeout: Duration) -> std::io::Result<Option<Vec<u8>>> {
    let ts = Timespec {
        tv_sec: timeout.as_secs() as _,
        tv_nsec: timeout.subsec_nanos() as _,
    };
    let mut fds = [PollFd::new(fd, PollFlags::IN)];
    if poll(&mut fds, Some(&ts))? == 0 {
        return Ok(None);
    }
    let mut buf = vec![0u8; MAX_PACKET];
    let n = match rustix::io::read(fd, &mut buf) {
        Ok(n) => n,
        Err(rustix::io::Errno::AGAIN) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if n == 0 {
        return Err(std::io::ErrorKind::UnexpectedEof.into());
    }
    buf.truncate(n);
    Ok(Some(buf))
}

/// Writes one H4 packet to `fd` (all of it or an error).
fn write_packet(fd: &OwnedFd, packet: &[u8]) -> std::io::Result<()> {
    let n = rustix::io::write(fd, packet)?;
    if n != packet.len() {
        return Err(std::io::ErrorKind::WriteZero.into());
    }
    Ok(())
}

/// An open raw HCI node of the Ultar kernel (`dev/ultar/hciN`).
pub struct RawNode {
    fd: OwnedFd,
}

impl RawNode {
    /// Opens the node at `path` (`EACCES` without the right, `EBUSY` when open elsewhere).
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let fd = rustix::fs::open(
            path,
            rustix::fs::OFlags::RDWR | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NONBLOCK,
            rustix::fs::Mode::empty(),
        )?;
        Ok(Self { fd })
    }

    /// One H4 packet, waiting at most `timeout`; `None` when none came.
    pub fn read(&self, timeout: Duration) -> std::io::Result<Option<Vec<u8>>> {
        read_packet(&self.fd, timeout)
    }

    /// Writes one H4 packet.
    pub fn write(&self, packet: &[u8]) -> std::io::Result<()> {
        write_packet(&self.fd, packet)
    }
}

/// `struct sockaddr_hci { sa_family_t hci_family; unsigned short hci_dev, hci_channel; }`.
#[repr(C)]
struct SockaddrHci {
    family: u16,
    dev: u16,
    channel: u16,
}

// SAFETY: `with_sockaddr` hands out a pointer to a live, fully initialised
// `sockaddr_hci` with its exact size, valid for the duration of the closure.
unsafe impl SocketAddrArg for SockaddrHci {
    unsafe fn with_sockaddr<R>(
        &self,
        f: impl FnOnce(*const SocketAddrOpaque, SocketAddrLen) -> R,
    ) -> R {
        let len = std::mem::size_of::<SockaddrHci>() as SocketAddrLen;
        f((self as *const SockaddrHci).cast(), len)
    }
}

/// An open HCI user channel.
pub struct UserChannel {
    fd: OwnedFd,
}

impl UserChannel {
    /// Opens the user channel of controller `dev`.
    pub fn open(dev: u16) -> std::io::Result<Self> {
        let proto = NonZeroU32::new(BTPROTO_HCI).map(Protocol::from_raw);
        let fd = rustix::net::socket_with(
            AddressFamily::BLUETOOTH,
            SocketType::RAW,
            rustix::net::SocketFlags::CLOEXEC,
            proto,
        )?;
        let addr = SockaddrHci {
            family: AddressFamily::BLUETOOTH.as_raw(),
            dev,
            channel: HCI_CHANNEL_USER,
        };
        rustix::net::bind(&fd, &addr)?;
        Ok(Self { fd })
    }

    /// One H4 packet, waiting at most `timeout`; `None` when none came.
    pub fn read(&self, timeout: Duration) -> std::io::Result<Option<Vec<u8>>> {
        read_packet(&self.fd, timeout)
    }

    /// Writes one H4 packet.
    pub fn write(&self, packet: &[u8]) -> std::io::Result<()> {
        write_packet(&self.fd, packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controllers_are_listed_from_sysfs() {
        let root = std::env::temp_dir().join(format!("hal-bt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(controllers(&root).is_empty());
        for n in ["hci1", "hci0", "hci0:12", "rfkill"] {
            std::fs::create_dir_all(root.join("sys/class/bluetooth").join(n)).unwrap();
        }
        assert_eq!(controllers(&root), vec![0, 1]);
        assert_eq!(std::mem::size_of::<SockaddrHci>(), 6);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ultar_nodes_are_listed_by_number() {
        let root = std::env::temp_dir().join(format!("hal-bt-node-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(ultar_nodes(&root).is_empty());
        std::fs::create_dir_all(root.join("dev/ultar/part")).unwrap();
        for n in ["hci2", "hci0", "uinput", "hcix", "hci10"] {
            std::fs::write(root.join("dev/ultar").join(n), b"").unwrap();
        }
        let names: Vec<String> = ultar_nodes(&root)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["hci0", "hci2", "hci10"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_raw_node_carries_whole_packets_and_times_out() {
        let root = std::env::temp_dir().join(format!("hal-bt-fifo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let fifo = root.join("hci0");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o600),
            0,
        )
        .unwrap();
        let node = RawNode::open(&fifo).unwrap();
        assert!(node.read(Duration::from_millis(20)).unwrap().is_none());
        node.write(&[1, 3, 0x0c, 0]).unwrap();
        let got = node.read(Duration::from_millis(500)).unwrap();
        assert_eq!(got.as_deref(), Some(&[1u8, 3, 0x0c, 0][..]));
        assert!(RawNode::open(&root.join("missing")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
