//! # ultar-update – the system updater of Ultar OS
//!
//! **Area:** system core (area 3 of the seven areas, plan §3.0). Design, threat model, formats
//! and state machine: `docs/SYSTEM-UPDATE.md`. Closes review finding S16.
//!
//! ## What it guarantees
//!
//! * **Nothing unsigned is acted on.** A release is described by a manifest with one canonical
//!   byte form, signed with Ed25519 under the domain `ultar-update-v1` by a key from the
//!   device's own update key set ([`keys`], [`sign`], [`verify`]). The server only says where
//!   bytes are.
//! * **No way back to an old release.** The manifest's rollback index must not be below the
//!   index stored on the device, which only grows ([`monotonic`]) and is raised only after the
//!   new release has started and passed its health checks ([`commit`]).
//! * **No half-applied state.** The running slot is never written. The other slot is
//!   unbootable from before its first byte until after a complete read-back ([`apply`]); every
//!   state file is replaced atomically; operations are "crash only" ([`fault`]).
//! * **A release that does not start does not stay.** It is started "once" with a boot counter;
//!   the boot code returns to the old slot by itself ([`bootctl`], [`slottable`]).
//! * **Same checks online and offline.** A download and a bundle from a stick go through the
//!   same staging code ([`stage`], [`acquire`]).
//!
//! ## Parts
//!
//! * [`engine::Engine`] – the synchronous core over a [`bootctl::BootControl`] backend
//!   ([`filectl::FileBootControl`] is the complete file-based reference backend).
//! * [`service::Core`] + [`busfront`] – the bus service `update` ([`contract`]) with policy,
//!   permissions (default deny) and progress events; [`daemon`] is the `ultar-updated` program.
//! * [`cli`] – the `uupdate` command; [`signtool`] / [`signcli`] – the publisher's
//!   `uupdate-sign`.
//! * [`msg`] – message catalogues (Arabic first, English, German); every state and error is a
//!   key ([`error::UpdateError::key`]).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

pub mod acquire;
pub mod apply;
pub mod args;
pub mod bootctl;
pub mod bundle;
pub mod busfront;
pub mod cli;
pub mod clock;
pub mod commit;
pub mod config;
pub mod contract;
pub mod daemon;
pub mod engine;
pub mod error;
pub mod fault;
pub mod filectl;
pub mod fsutil;
pub mod health;
pub mod history;
pub mod jobs;
pub mod journal;
pub mod keys;
pub mod manifest;
pub mod monotonic;
pub mod msg;
pub mod net;
pub mod policy;
pub mod reject;
pub mod service;
pub mod sign;
pub mod signcli;
pub mod signtool;
pub mod slottable;
pub mod source;
pub mod stage;
pub mod verify;
pub mod version;
