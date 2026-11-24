//! Hardware-independent NVIDIA kernel interfaces.
//!
//! The Quadro driver uses these contracts once the device-specific GSP transport is available.
//! They deliberately keep firmware transport, memory ownership, atomic display state and GPU
//! submission separate so an incomplete backend cannot report a successful mode set or submit.

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
