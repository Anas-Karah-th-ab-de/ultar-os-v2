//! `ultar-netd` – the network service of Ultar OS (area 4, system services).
//!
//! Publishes `net.state` (retained): the wired link and its speed, the Wi-Fi interface
//! and its radio, airplane mode; every wired interface and the Wi-Fi interface on their own
//! (name, MAC, driver, address, router, DNS, speed, state, [`wired`]) and whether the
//! internet is reachable ([`internet`]); and `net.wifi` (retained): the Wi-Fi connection with
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
//!
//! Wired interfaces: on the Ultar kernel the kernel holds the DHCP lease (renewed at T1,
//! asked again after the cable comes back) and takes fixed addresses (`net.set_ipv4`, kept
//! below `--state` and given back after a restart); their name servers go to `resolv.conf`
//! after the VPN's and Wi-Fi's.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod calls;
pub mod dns;
pub mod internet;
pub mod service;
pub mod wifi;
pub mod wired;

pub use service::NetService;
