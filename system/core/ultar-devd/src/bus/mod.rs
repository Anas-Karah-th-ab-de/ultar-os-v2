//! The daemon's side of `ultar-bus`.
//!
//! * [`sink::BusSink`] – the [`crate::CapabilitySink`] that publishes
//!   `devices.capabilities` (retained, the complete set) and `devices.driver_state`.
//! * [`convert`] – manager views and errors as contract types of
//!   `ultar_bus::messages::device_service`.
//! * The bus service `devices` itself is [`crate::daemon::Daemon::serve`].

pub mod convert;
pub mod sink;

pub use sink::{BusSink, MemoryPublisher, Published, Publisher, UnservedDevice};
