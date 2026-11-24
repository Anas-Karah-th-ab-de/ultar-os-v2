//! `ultar-vpnd` – the VPN of Ultar OS (area 4, system services), served on the
//! bus as `vpn` by the network service `ultar-netd` (same process: busd and
//! logd were at the kernel's limit of 16 groups when it was written, so no
//! own service user; the limit is 32 now).
//!
//! WireGuard tunnels: a standard `.conf` file is imported and checked
//! ([`conf`]: commands such as `PostUp` are refused, never run), kept with
//! its keys in a file of mode 0600 below the state directory, and switched
//! on and off from the shell's quick setting `qs.vpn` or the Settings app
//! (`vpn.set`). The protocol (handshake, keys, timers, ChaCha20-Poly1305) is
//! boringtun's reviewed implementation; this crate adds the TUN interface,
//! addresses, routes and the cryptokey-routing check on incoming packets
//! ([`tunnel`]). The tunnel's DNS servers go to `vpn.state`; the network
//! service writes them first into `resolv.conf`.
//!
//! Where the kernel has no TUN device (the Ultar kernel today) `vpn.state`
//! says `available: false, unavailable: NoTun` and every call answers
//! `Unavailable`.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod conf;
pub mod service;
pub mod tunnel;

pub use service::VpnService;
