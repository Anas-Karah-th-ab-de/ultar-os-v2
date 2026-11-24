//! The system services behind the shell's controls (docs/integration/system-services.md) as one
//! port: each [`Call`] of a control is one bus call; [`ServicesPort::call`] answers what the
//! shell shows next. The bus implementation is in `ports/bus.rs`; tests use fakes.

use super::PortError;
use crate::model::services::Call;
use ultar_bus::messages::calendar::DayEvent;

/// What a call gave back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Done; nothing to show.
    Done,
    /// The service could not (no device, unsupported, busy, refused): the shell says so.
    NotDone,
    /// `audio.outputs`: (id, name, active).
    Outputs(Vec<(String, String, bool)>),
    /// `calendar.day`: the events.
    Day(Vec<DayEvent>),
    /// `capture.screenshot`: saved (the file's path).
    Saved(String),
    /// `bluetooth.devices`: the devices.
    BtDevices(Vec<ultar_bus::messages::bluetooth::Device>),
    /// `net.networks`: the access points.
    Networks(Vec<ultar_bus::messages::net::NetworkInfo>),
    /// `net.known`: the saved networks.
    Known(Vec<String>),
    /// `net.join`: how it went.
    Joined(ultar_bus::messages::net::JoinOutcome),
    /// What became of "continue on another device".
    Sent(ultar_bus::messages::mesh::SendOutcome),
}

/// The calls of the shell's controls.
pub trait ServicesPort: Send + Sync {
    /// Runs one call.
    fn call(&self, call: &Call) -> Result<Answer, PortError>;
}
