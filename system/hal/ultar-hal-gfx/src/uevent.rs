//! Kernel device events (`NETLINK_KOBJECT_UEVENT`, Linux and the Ultar
//! kernel): the compositor rescans its input devices when an input or USB
//! device comes or goes.

use crate::GfxError;
use rustix::net::netlink::{self, SocketAddrNetlink};
use rustix::net::{bind, recv, socket_with, AddressFamily, RecvFlags, SocketFlags, SocketType};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

/// Largest uevent message read (the rest of a longer one is dropped).
const MESSAGE: usize = 8192;

/// A subscription to kernel device events.
#[derive(Debug)]
pub struct UeventSocket {
    fd: OwnedFd,
}

/// What one event says, as far as input is concerned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Uevent {
    /// `add`, `remove`, `bind`, `unbind`, `change`.
    pub action: String,
    /// `SUBSYSTEM=` value.
    pub subsystem: String,
}

impl Uevent {
    /// Parses `ACTION@DEVPATH\0KEY=VALUE\0…`; `None` for anything else
    /// (libudev messages start with `libudev\0` and are ignored).
    pub fn parse(message: &[u8]) -> Option<Uevent> {
        let mut parts = message.split(|&b| b == 0).filter(|p| !p.is_empty());
        let head = std::str::from_utf8(parts.next()?).ok()?;
        let (action, _) = head.split_once('@')?;
        let mut subsystem = String::new();
        for part in parts {
            if let Some(v) = part.strip_prefix(b"SUBSYSTEM=") {
                subsystem = String::from_utf8_lossy(v).into_owned();
            }
        }
        Some(Uevent {
            action: action.to_owned(),
            subsystem,
        })
    }

    /// True if input devices may have changed (input nodes, USB devices).
    pub fn concerns_input(&self) -> bool {
        matches!(self.subsystem.as_str(), "input" | "usb" | "hid")
            && matches!(self.action.as_str(), "add" | "remove" | "bind" | "unbind")
    }
}

impl UeventSocket {
    /// Subscribes (non-blocking). Fails where the kernel has no uevents.
    pub fn open() -> Result<UeventSocket, GfxError> {
        let flags = SocketFlags::CLOEXEC | SocketFlags::NONBLOCK;
        let fd = socket_with(
            AddressFamily::NETLINK,
            SocketType::DGRAM,
            flags,
            Some(netlink::KOBJECT_UEVENT),
        )?;
        bind(&fd, &SocketAddrNetlink::new(0, 1))?;
        Ok(UeventSocket { fd })
    }

    /// The descriptor for the caller's `poll` loop.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }

    /// Reads every pending event; true if any concerned input devices.
    pub fn input_changed(&self) -> bool {
        let mut buf = vec![0u8; MESSAGE];
        let mut changed = false;
        for _ in 0..256 {
            match recv(&self.fd, &mut buf, RecvFlags::DONTWAIT) {
                Ok((n, _)) if n > 0 => {
                    let n = n.min(buf.len());
                    changed |= Uevent::parse(&buf[..n]).is_some_and(|e| e.concerns_input());
                }
                _ => break,
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_parsed() {
        let m = b"add@/devices/pci0000:00/usb1/1-1\0ACTION=add\0DEVPATH=/devices/x\0SUBSYSTEM=usb\0SEQNUM=4\0";
        let e = Uevent::parse(m).unwrap();
        assert_eq!(e.action, "add");
        assert_eq!(e.subsystem, "usb");
        assert!(e.concerns_input());
        let pci = Uevent::parse(b"change@/devices/p\0SUBSYSTEM=pci\0").unwrap();
        assert!(!pci.concerns_input());
        assert!(Uevent::parse(b"libudev\0\xfe\xed").is_none());
        assert!(Uevent::parse(b"").is_none());
        let input =
            Uevent::parse(b"remove@/devices/virtual/input/input9/event5\0SUBSYSTEM=input\0")
                .unwrap();
        assert!(input.concerns_input());
        // Opening may be refused in a sandbox; when it works nothing is pending.
        if let Ok(s) = UeventSocket::open() {
            let _ = s.fd();
            let _ = s.input_changed();
        }
    }
}
