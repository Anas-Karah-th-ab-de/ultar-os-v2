//! Hardware-independent NVIDIA kernel interfaces.
//!
//! The Quadro driver uses these contracts once the device-specific GSP transport is available.
//! They deliberately keep firmware transport, memory ownership, atomic display state and GPU
//! submission separate so an incomplete backend cannot report a successful mode set or submit.
//!
//! Nothing calls them until that transport exists: the whole module is allowed to be unused
//! (docs/OFFEN-WARNUNGEN.md). Remove the line below with the first caller.
#![allow(dead_code)]

pub mod compute;
pub mod gsp;
pub mod kms;
pub mod memory;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidArgument,
    InvalidState,
    NoMemory,
    NotReady(&'static str),
    Unsupported(&'static str),
    Timeout,
}

pub type Result<T> = core::result::Result<T, Error>;
