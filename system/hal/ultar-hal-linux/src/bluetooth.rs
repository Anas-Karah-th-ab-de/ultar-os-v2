//! Bluetooth controllers: the list (`sys/class/bluetooth/hciN`) and the raw HCI **user
//! channel** of one controller (`AF_BLUETOOTH`/`BTPROTO_HCI` bound to
//! `HCI_CHANNEL_USER`): the kernel's own host stack lets go of the controller and every
//! H4 packet goes to and from the one process holding the channel (needs
//! `CAP_NET_ADMIN`; the controller must be down – `EBUSY` otherwise).
//!
//! The Ultar kernel has no Bluetooth socket family: there `socket()` fails and the
//! service reports "no Bluetooth device".
#![allow(unsafe_code)]

use rustix::event::{poll, PollFd, PollFlags, Timespec};
use rustix::net::addr::{SocketAddrArg, SocketAddrLen, SocketAddrOpaque};
use rustix::net::{AddressFamily, Protocol, SocketType};
use std::num::NonZeroU32;
use std::os::fd::OwnedFd;
use std::path::Path;
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
        let ts = Timespec {
            tv_sec: timeout.as_secs() as _,
            tv_nsec: timeout.subsec_nanos() as _,
        };
        let mut fds = [PollFd::new(&self.fd, PollFlags::IN)];
        if poll(&mut fds, Some(&ts))? == 0 {
            return Ok(None);
        }
        let mut buf = vec![0u8; MAX_PACKET];
        let n = rustix::io::read(&self.fd, &mut buf)?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf.truncate(n);
        Ok(Some(buf))
    }

    /// Writes one H4 packet.
    pub fn write(&self, packet: &[u8]) -> std::io::Result<()> {
        let n = rustix::io::write(&self.fd, packet)?;
        if n != packet.len() {
            return Err(std::io::ErrorKind::WriteZero.into());
        }
        Ok(())
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
}
