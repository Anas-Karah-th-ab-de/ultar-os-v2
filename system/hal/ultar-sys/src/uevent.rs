//! Kernel device events (uevents) from an `AF_NETLINK` socket of protocol
//! `NETLINK_KOBJECT_UEVENT` – the same on the Ultar kernel
//! (`docs/integration/kernel-device-interfaces.md`) and on Linux.
//!
//! Safe through `rustix::net`. A message is `ACTION@DEVPATH\0KEY=VALUE\0…`;
//! messages of the user-space device manager on Linux (`libudev` header) are
//! skipped. `SEQNUM` counts the kernel's events without gaps: a reader that
//! sees a gap lost events and reads the device tree again.

use crate::Errno;
use rustix::net::netlink::{self, SocketAddrNetlink};
use rustix::net::{AddressFamily, RecvFlags, SocketFlags, SocketType};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::time::Duration;

/// Largest message read (the kernel's events are far shorter).
pub const MAX_MESSAGE: usize = 8192;
/// Most `KEY=VALUE` pairs kept from one message.
pub const MAX_VARS: usize = 64;

/// One device event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Uevent {
    /// `add`, `remove`, `bind`, `unbind`, `change`, …
    pub action: String,
    /// Path of the device below `/sys` (`/bus/pci/devices/0000:00:04.0` on the Ultar kernel).
    pub devpath: String,
    /// `pci`, `usb`, …
    pub subsystem: String,
    /// The kernel's event number, if it sent one.
    pub seqnum: Option<u64>,
    /// All `KEY=VALUE` pairs in order.
    pub vars: Vec<(String, String)>,
}

impl Uevent {
    /// Parses one message; `None` for a message that is not a kernel uevent.
    pub fn parse(message: &[u8]) -> Option<Self> {
        let mut parts = message.split(|&b| b == 0).filter(|p| !p.is_empty());
        let head = std::str::from_utf8(parts.next()?).ok()?;
        let (action, devpath) = head.split_once('@')?;
        let mut event = Self {
            action: action.to_string(),
            devpath: devpath.to_string(),
            subsystem: String::new(),
            seqnum: None,
            vars: Vec::new(),
        };
        for part in parts.take(MAX_VARS) {
            let Some((key, value)) = std::str::from_utf8(part)
                .ok()
                .and_then(|p| p.split_once('='))
            else {
                continue;
            };
            match key {
                "SUBSYSTEM" => event.subsystem = value.to_string(),
                "SEQNUM" => event.seqnum = value.parse().ok(),
                _ => {}
            }
            event.vars.push((key.to_string(), value.to_string()));
        }
        Some(event)
    }

    /// The value of `key`, if the message carried it.
    pub fn var(&self, key: &str) -> Option<&str> {
        self.vars
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// A socket that receives the kernel's device events from its creation on.
#[derive(Debug)]
pub struct UeventSocket {
    fd: OwnedFd,
}

impl UeventSocket {
    /// Opens the socket and joins the kernel's event group (group 1).
    pub fn open() -> Result<Self, Errno> {
        let fd = rustix::net::socket_with(
            AddressFamily::NETLINK,
            SocketType::RAW,
            SocketFlags::CLOEXEC,
            Some(netlink::KOBJECT_UEVENT),
        )?;
        rustix::net::bind(&fd, &SocketAddrNetlink::new(0, 1))?;
        Ok(Self { fd })
    }

    /// Waits for the next kernel event at most `timeout` (`None`: no limit);
    /// `Ok(None)` when the time passed. Messages that are not kernel events
    /// are skipped.
    pub fn next(&self, timeout: Option<Duration>) -> Result<Option<Uevent>, Errno> {
        let mut buf = vec![0u8; MAX_MESSAGE];
        loop {
            let ready = crate::poll_readable(&[self.fd.as_fd()], timeout)?;
            if !ready.first().copied().unwrap_or(false) {
                return Ok(None);
            }
            let (n, _) = match rustix::net::recv(&self.fd, &mut buf[..], RecvFlags::empty()) {
                Ok(got) => got,
                Err(rustix::io::Errno::INTR | rustix::io::Errno::AGAIN) => continue,
                Err(errno) => return Err(errno.into()),
            };
            if n == 0 {
                return Err(Errno(rustix::io::Errno::PIPE.raw_os_error()));
            }
            if let Some(event) = Uevent::parse(&buf[..n.min(buf.len())]) {
                return Ok(Some(event));
            }
        }
    }
}

impl AsFd for UeventSocket {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}
