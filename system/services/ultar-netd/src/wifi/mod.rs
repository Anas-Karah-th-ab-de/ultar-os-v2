//! Wi-Fi networks: scanning, joining open and WPA2-Personal networks with the
//! service's own supplicant (`ultar_netproto::wpa`), DHCP and DNS, saved
//! networks and automatic reconnection.
//!
//! [`core::WifiCore`] is the logic (no threads, time given in ms);
//! [`worker`] runs it on its own thread next to the kernel sockets, because
//! the handshake must answer the access point within its timeouts, and tells
//! the service loop when the connection changes.

pub mod core;
pub mod driver;
mod join;
mod link;
pub mod saved;
pub mod worker;

pub use self::core::{Command, ScanAnswer, Seen, WifiCore};
