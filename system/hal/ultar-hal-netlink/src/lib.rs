//! `ultar-hal-netlink` – the kernel's network interfaces on Linux (area 2,
//! hardware layer).
//!
//! | Part | Module |
//! |---|---|
//! | Netlink messages and attributes (build, read hostile bytes) | [`attr`] |
//! | Netlink socket: requests, acknowledgements, dumps, events | [`socket`] |
//! | Generic netlink: family and multicast group ids | [`genl`] |
//! | nl80211: Wi-Fi interfaces, scan, connect, keys, EAPOL over the control port | [`nl80211`] |
//! | rtnetlink: links, addresses, routes | [`rtnl`] |
//! | TUN devices (VPN) | [`tun`] |
//! | The DHCP client socket (UDP 68 bound to one interface) | [`udp`] |
//!
//! Where the kernel has none of this (the Ultar kernel today: no netlink
//! families for Wi-Fi, no TUN), every entry point answers
//! [`NetError::Unavailable`] – the services report that honestly.
//! The only `unsafe` code is in [`sys`]: two socket options and one ioctl on
//! fixed-size buffers.
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs, unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod attr;
pub mod genl;
pub mod nl80211;
pub mod rtnl;
pub mod socket;
mod sys;
pub mod tun;
pub mod udp;

/// Why a kernel network call failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetError {
    /// The kernel offers no such interface (no netlink family, no TUN).
    Unavailable,
    /// The kernel refused (errno).
    Kernel(i32),
    /// The kernel's answer was malformed.
    Malformed,
    /// No answer in time.
    Timeout,
    /// The named device does not exist.
    NoDevice,
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::Unavailable => f.write_str("not offered by the kernel"),
            NetError::Kernel(e) => write!(f, "kernel error {e}"),
            NetError::Malformed => f.write_str("malformed kernel answer"),
            NetError::Timeout => f.write_str("timeout"),
            NetError::NoDevice => f.write_str("no such device"),
        }
    }
}

impl std::error::Error for NetError {}

impl From<rustix::io::Errno> for NetError {
    fn from(e: rustix::io::Errno) -> Self {
        match e {
            rustix::io::Errno::AFNOSUPPORT
            | rustix::io::Errno::PROTONOSUPPORT
            | rustix::io::Errno::NOENT
            | rustix::io::Errno::NODEV
            | rustix::io::Errno::NOSYS => NetError::Unavailable,
            rustix::io::Errno::AGAIN => NetError::Timeout,
            other => NetError::Kernel(other.raw_os_error()),
        }
    }
}
