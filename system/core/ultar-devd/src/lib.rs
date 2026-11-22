//! Runtime logic of the device daemon `ultar-devd` (plan §5.4).
//!
//! **Area:** system core (plan §3.0, area 3). This crate is a pure library: it touches
//! no hardware and no files. Everything outside is reached through traits –
//! [`DriverLoader`] (load/unload a driver), [`HealthCheck`] (does the device work?),
//! [`ChoiceStore`] (persist decisions) and [`CapabilitySink`] (publish capabilities on
//! the bus). The daemon binary and the HAL supply the implementations.
//!
//! Flow for a device event:
//!
//! 1. candidates from the depot index ([`ultar_driverpkg::candidates`]),
//! 2. the remembered driver first, then the others best-first,
//! 3. load → health check; on failure the driver is quarantined with a reason and the
//!    next candidate is tried,
//! 4. the working choice is remembered, so the next boot loads it directly; if it no
//!    longer works the search runs again and the store is corrected,
//! 5. a [`CapabilityChange`] is emitted.
//!
//! The per-device life cycle is an explicit state machine ([`fsm`]). The manual
//! operations of the settings panel "Drivers & hardware" (list, switch, roll back,
//! clear quarantine, rescan) are methods of [`DeviceManager`].

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![deny(missing_docs)]

pub mod error;
pub mod fsm;
pub mod manager;
mod ops;
mod probe;
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
