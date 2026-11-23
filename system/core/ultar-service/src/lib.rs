//! # ultar-service – the runtime every Ultar daemon uses
//!
//! **Area:** system core (area 3 of the seven areas, plan §3.0). This crate
//! closes review block "B – services": until now no daemon reacted to stop
//! signals, none limited itself, the bus could not identify its peers and
//! there was no log service to write to.
//!
//! | Part | Module |
//! |---|---|
//! | `SIGTERM`/`SIGINT` → orderly stop with deadline, `SIGHUP` → reload, without a C library | [`signal`] |
//! | States `starting → ready → stopping → stopped`, stop and reload hooks | [`lifecycle`] |
//! | Readiness protocol with the supervisor (both sides) | [`ready`] |
//! | Self-limits at start: descriptors, umask, root, limits, kernel rights | [`harden`] |
//! | Kernel peer credentials (`SO_PEERCRED`) and the uid → identity map for the bus | [`peercred`] |
//! | Bus connection with back-off that restores retained topics after a broker restart | [`session`] |
//! | Secrets from checked files or inherited descriptors; a type that cannot be printed | [`secret`] |
//! | Logging API: typed records, rate limit, bounded buffer, never blocks | [`log`] |
//! | Binding an own socket without ever deleting a non-socket | [`socket`] |
//! | All of it in one call for `main` | [`runtime`] |
//!
//! ## `unsafe`
//!
//! None: the crate forbids `unsafe` code. The system calls `std` does not
//! offer (signal mask and `signalfd`, `SO_PEERCRED`, limits, `prctl`,
//! inherited descriptors) come from the HAL crate `ultar-sys`, which uses
//! `rustix` where that has a safe function and documents the rest.
//!
//! ## Portability
//!
//! x86_64 and aarch64, Linux system call ABI. What the Ultar kernel does
//! not implement yet is listed in [`harden`] and [`peercred`]; in each
//! case the code detects the missing mechanism and fails closed or reports
//! it – it never pretends.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

pub mod exit;
pub mod harden;
pub mod lifecycle;
pub mod log;
pub mod peercred;
pub mod ready;
pub mod rights;
pub mod runtime;
pub mod secret;
pub mod session;
pub mod signal;
pub mod socket;

pub use harden::{
    Hardening, HardeningError, HardeningReport, Limits, Outcome, Platform, Right, RightSet, Step,
};
pub use lifecycle::{Lifecycle, Signal, State, StopCause, Wake};
pub use peercred::{KernelCredentials, PeerCredError, UidMap};
pub use ready::{ReadyError, ReadyEvent, ReadyNotifier, ReadyReader, READY_FD_ENV};
pub use runtime::{Runtime, ServiceSpec, StartError};
pub use secret::{
    read_secret_file, read_secret_file_limited, read_secret_owned, read_secret_source,
    take_inherited_secret, Secret, SecretError, SecretFilePolicy, SecretSource,
    MAX_SECRET_TABLE_LEN,
};
pub use session::{BusSession, SessionOptions};
pub use ultar_sys::{Errno, Resource};

use ultar_sys as sys;

/// Limits of this process as the kernel reports them now: `(soft, hard)`.
/// Lets a service or a test read back what hardening set.
pub fn current_limit(resource: Resource) -> Result<(u64, u64), Errno> {
    sys::limit(resource)
}

/// Effective user id of this process.
pub fn effective_uid() -> u32 {
    sys::euid()
}

/// Real group id of this process.
pub fn group_id() -> u32 {
    sys::gid()
}
