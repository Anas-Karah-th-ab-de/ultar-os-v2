//! `ultar-mesh` – the device mesh of Ultar OS (area 4, system services).
//!
//! The person's own devices find each other in the local network, are paired once by a
//! six-digit code and then hand work over: an activity ("continue this on the other
//! device"), files and the clipboard. Every connection is end-to-end encrypted and both
//! sides prove their key. Nothing arrives unasked: the receiving device always asks its
//! person, and the sending device asks which device is meant when several can take it.
//!
//! The mesh knows devices by what they can do (screen, sound, files, apps …), so a TV or
//! a speaker joins with the same protocol and is offered only what it can take.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod acting;
pub mod clipboard;
mod codec;
pub mod compute;
pub mod device;
pub mod handoff;
mod lending;
pub mod model;
pub mod net;
pub mod node;
pub mod offers;
mod pairing;
mod received;
pub mod relay;
pub mod remote;
pub mod runtime;
pub mod secure;
pub mod service;
pub mod transfer;
pub mod trust;
pub mod view;
pub mod wire;

pub use service::MeshService;
