//! The phone's two outside connections: the call service ([`CallPort`], over the bus in
//! production: [`BusPort`]) and the platform ([`Host`]: clipboard, window, other apps).
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::click::WindowOp;
use std::fmt;
use std::time::Duration;
use ultar_bus::messages::call::{
    Answer, Block, CallError, CallState, ClearLog, Decline, Dial, Dtmf, Forget, Hangup, Hold, Log,
    LogEntry, Mute, NextCamera, SendText, SetOutput, SetVideo, Share, Snapshot,
};
use ultar_bus::{BusError, Client, ErrorCode, TypedSubscription};

/// A request to the call service.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Request {
    /// `call.dial`.
    Dial(Dial),
    /// `call.answer`.
    Answer(Answer),
    /// `call.decline`.
    Decline(Decline),
    /// `call.hangup`.
    Hangup(Hangup),
    /// `call.hold`.
    Hold(Hold),
    /// `call.mute`.
    Mute(Mute),
    /// `call.output`.
    Output(SetOutput),
    /// `call.dtmf`.
    Dtmf(Dtmf),
    /// `call.video`.
    Video(SetVideo),
    /// `call.camera`.
    Camera(NextCamera),
    /// `call.share`.
    Share(Share),
    /// `call.text`.
    Text(SendText),
    /// `call.forget`.
    Forget(Forget),
    /// `call.block`.
    Block(Block),
    /// `call.clear`.
    Clear(ClearLog),
}

/// Why a request did not work.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PortError {
    /// The call service refused with this error.
    Call(CallError),
    /// The call service could not be reached.
    Unreachable,
}

impl fmt::Display for PortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortError::Call(e) => write!(f, "{e}"),
            PortError::Unreachable => f.write_str("call service not reachable"),
        }
    }
}

impl std::error::Error for PortError {}

/// Connection to the call service.
pub trait CallPort {
    /// Sends one request; `Ok(Some(id))` for a dial (the new call), `Ok(None)` otherwise.
    fn send(&mut self, request: Request) -> Result<Option<u32>, PortError>;
    /// The call history, newest first.
    fn log(&mut self, missed_only: bool) -> Result<Vec<LogEntry>, PortError>;
}

/// What the platform does for the phone.
pub trait Host {
    /// Puts text on the clipboard.
    fn copy_text(&mut self, text: &str) -> bool;
    /// Minimises, maximises or closes the window.
    fn window(&mut self, op: WindowOp);
    /// Opens the settings page of the phone.
    fn open_settings(&mut self);
    /// Opens Contacts with a new contact for this number.
    fn create_contact(&mut self, number: &str);
}

/// The call service over the bus.
pub struct BusPort {
    bus: Client,
    timeout: Duration,
}

fn bus_err(e: BusError) -> PortError {
    match e {
        BusError::Remote(ErrorCode::Service(code)) => CallError::from_code(code)
            .map(PortError::Call)
            .unwrap_or(PortError::Unreachable),
        _ => PortError::Unreachable,
    }
}

impl BusPort {
    /// A port over `bus`; every request waits at most `timeout`.
    pub fn new(bus: Client, timeout: Duration) -> BusPort {
        BusPort { bus, timeout }
    }

    /// Subscription to `call.state` (the retained state arrives first).
    pub fn states(&self) -> Result<TypedSubscription<CallState>, PortError> {
        self.bus.subscribe::<CallState>().map_err(bus_err)
    }

    /// The current state, asked directly.
    pub fn snapshot(&self) -> Result<CallState, PortError> {
        self.bus.call(&Snapshot, self.timeout).map_err(bus_err)
    }
}

impl CallPort for BusPort {
    fn send(&mut self, request: Request) -> Result<Option<u32>, PortError> {
        let t = self.timeout;
        let b = &self.bus;
        let done =
            |r: Result<ultar_bus::messages::call::Done, BusError>| r.map(|_| None).map_err(bus_err);
        match request {
            Request::Dial(d) => b.call(&d, t).map(|r| Some(r.id)).map_err(bus_err),
            Request::Answer(r) => done(b.call(&r, t)),
            Request::Decline(r) => done(b.call(&r, t)),
            Request::Hangup(r) => done(b.call(&r, t)),
            Request::Hold(r) => done(b.call(&r, t)),
            Request::Mute(r) => done(b.call(&r, t)),
            Request::Output(r) => done(b.call(&r, t)),
            Request::Dtmf(r) => done(b.call(&r, t)),
            Request::Video(r) => done(b.call(&r, t)),
            Request::Camera(r) => done(b.call(&r, t)),
            Request::Share(r) => done(b.call(&r, t)),
            Request::Text(r) => done(b.call(&r, t)),
            Request::Forget(r) => done(b.call(&r, t)),
            Request::Block(r) => done(b.call(&r, t)),
            Request::Clear(r) => done(b.call(&r, t)),
        }
    }

    fn log(&mut self, missed_only: bool) -> Result<Vec<LogEntry>, PortError> {
        self.bus
            .call(&Log { missed_only }, self.timeout)
            .map(|r| r.entries)
            .map_err(bus_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_errors_map_to_port_errors() {
        let e = bus_err(BusError::Remote(ErrorCode::Service(
            CallError::TooManyCalls.code(),
        )));
        assert_eq!(e, PortError::Call(CallError::TooManyCalls));
        assert_eq!(
            bus_err(BusError::Remote(ErrorCode::Service(999))),
            PortError::Unreachable
        );
        assert_eq!(bus_err(BusError::Timeout), PortError::Unreachable);
        assert_eq!(
            PortError::Unreachable.to_string(),
            "call service not reachable"
        );
        assert!(PortError::Call(CallError::UnknownCall)
            .to_string()
            .contains('1'));
    }
}
