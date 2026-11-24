//! `ultar-netd` – the network service of Ultar OS (area 4, system services).
//!
//! Publishes `net.state` (retained): the wired link and its speed, the Wi-Fi interface
//! and its radio, airplane mode; and `net.wifi` (retained): the Wi-Fi connection with
//! its address and DNS. Switches the Wi-Fi radio and airplane mode through the
//! kernel's radio switches (HAL [`ultar_hal_linux::rfkill`]); a hardware switch is
//! reported, never overridden. The wanted state is kept in a checked file and restored
//! at start.
//!
//! Wi-Fi networks ([`wifi`]): scan, join open and WPA2-Personal networks with the
//! service's own supplicant (nl80211 through `ultar-hal-netlink`, the 4-way handshake
//! of `ultar-netproto`), DHCP, DNS (`resolv.conf` below `--run`), saved networks (key
//! only, 0600) and automatic reconnection. WPA3-only and enterprise networks are listed
//! but not joined. Where the kernel has no Wi-Fi (the Ultar kernel today) the service
//! says so: `wifi_present: false`, calls answer `NoDevice`.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod calls;
pub mod dns;
pub mod service;
pub mod wifi;

pub use service::NetService;
