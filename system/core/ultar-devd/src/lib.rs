//! The device daemon `ultar-devd` (plan §5.4) and its command line client `udevctl`.
//!
//! **Area:** system core (plan §3.0, area 3). Hardware paths appear nowhere in this crate;
//! the system description comes from the HAL crate `ultar-hal-system`.
//!
//! The crate has two layers:
//!
//! **Pure logic** (no I/O) – [`DeviceManager`] with the per-device state machine ([`fsm`]),
//! reached from outside only through the traits [`DriverLoader`], [`HealthCheck`],
//! [`ChoiceStore`] and [`CapabilitySink`]. Flow for a device event:
//!
//! 1. candidates from the depot index ([`ultar_driverpkg::candidates`]),
//! 2. the remembered driver first, then the others best-first,
//! 3. load → health check; on failure the driver is quarantined with a reason and the
//!    next candidate is tried,
//! 4. the working choice is remembered, so the next boot loads it directly; if it no
//!    longer works the search runs again and the store is corrected,
//! 5. a [`CapabilityChange`] is emitted.
//!
//! **The running service** – the implementations of those traits and the daemon:
//!
//! * [`store::file`] – file-backed store (atomic, checksummed, self-recovering),
//! * [`builtin`] – loader and health check for drivers built into the kernel (stage A),
//! * [`bus`] – capability sink publishing `devices.capabilities` / `devices.driver_state`,
//! * [`daemon`] + [`service`] – start-up and the bus service `devices`
//!   (list, switch, rollback, clear_quarantine, rescan, shutdown),
//! * [`runner`] / [`cli`] – the binaries `ultar-devd` and `udevctl`, [`msg`] their texts.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![deny(missing_docs)]

pub mod access;
pub mod builtin;
pub mod bus;
pub mod caps;
pub mod cli;
pub mod daemon;
pub mod depot;
pub mod error;
pub mod fsm;
pub mod manager;
pub mod msg;
mod ops;
mod probe;
pub mod runner;
pub mod service;
pub mod sources;
pub mod store;
pub mod traits;
pub mod types;

pub use error::DevdError;
pub use fsm::{transition, InvalidTransition, Phase, Signal};
pub use manager::{Config, DeviceManager};
pub use ops::{AlternativeView, DeviceView, QuarantineFilter};
pub use store::{MemoryStore, StoredState};
pub use traits::{
    CapabilitySink, ChoiceStore, DriverLoader, HealthCheck, HealthVerdict, LoadError, StoreError,
};
pub use types::{
    Attempt, AttemptOutcome, Capability, CapabilityChange, ChangeKind, DeviceEvent, DriverKey,
    FailureReason, HealthFailure, LoadFailure, ProbeReport, QuarantineEntry,
};
