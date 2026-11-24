//! `ultar-netproto` – the network protocols of the network service, without
//! sockets, threads or clocks (area 3, system core).
//!
//! | Part | Module |
//! |---|---|
//! | Wi-Fi information elements of scan results: name, security | [`ie`] |
//! | WPA2-Personal: passphrase → PMK, PTK, EAPOL-Key frames, RSN element, 4-way and group-key handshake | [`wpa`] |
//! | DHCPv4 client: messages and the lease state machine | [`dhcp`] |
//!
//! Everything here is a state machine fed with bytes and a time in
//! milliseconds; the caller owns the sockets (`ultar-hal-netlink`) and the
//! clock. Ciphers are never implemented here: PBKDF2, HMAC-SHA1 and the AES
//! key wrap come from the RustCrypto crates, frame encryption (CCMP) is done
//! by the kernel driver with the keys this crate derives. Every input is
//! treated as hostile: lengths are checked before use, nothing panics, and
//! secrets are compared in constant time and wiped when dropped.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod dhcp;
pub mod ie;
pub mod wpa;

/// A 6-byte hardware (MAC) address.
pub type Mac = [u8; 6];
