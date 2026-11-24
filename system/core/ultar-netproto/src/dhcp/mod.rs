//! DHCPv4 client (RFC 2131, options RFC 2132): the messages and the lease
//! state machine. The caller sends the bytes from UDP port 68 (to the
//! broadcast address or the server), feeds every datagram it receives and
//! calls [`Client::poll`] at [`Client::next_deadline`].
//!
//! Not done: the address conflict check before using an offer (ARP probe,
//! RFC 5227) – a conflicting address is detected by the server's NAK on
//! renewal only.

pub mod client;
pub mod msg;

pub use client::{Action, Client, Lease};
pub use msg::{DhcpError, Message, MessageType};
