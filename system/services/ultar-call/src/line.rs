//! Backends of the call service: one [`Line`] per way (Ultar number, mobile network) and one
//! [`AudioRoute`] for microphone and output. The session ([`crate::session`]) drives them;
//! the test double is [`crate::fake`].
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::fmt;
use ultar_bus::messages::call::{Cellular, End, Failure, Media, Output, Ultar, Way};

/// Key of one call inside a line (chosen by the line).
pub type Key = u64;

/// Failure of a line or audio backend.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineError {
    /// The way cannot do this.
    Unsupported,
    /// The way is not available now (no network, signed out, …).
    Unavailable(Failure),
    /// The line does not know the call.
    UnknownCall,
    /// The backend failed.
    Backend,
}

impl fmt::Display for LineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LineError::Unsupported => f.write_str("not supported on this way"),
            LineError::Unavailable(why) => write!(f, "way unavailable: {why:?}"),
            LineError::UnknownCall => f.write_str("unknown call"),
            LineError::Backend => f.write_str("backend failure"),
        }
    }
}

impl std::error::Error for LineError {}

/// Availability report of a line.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Status {
    /// Mobile network state and the SIM slots with a card.
    Cellular(Cellular, Vec<u8>),
    /// Ultar-number state and the own number.
    Ultar(Ultar, Option<String>),
}

/// What a line reports.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LineEvent {
    /// A call comes in.
    Incoming {
        /// Key of the new call.
        key: Key,
        /// Its way.
        way: Way,
        /// The caller's number.
        number: String,
        /// Name from the contacts, if known.
        name: Option<String>,
        /// Video call offered (Ultar way).
        video: bool,
    },
    /// The other side rings (outgoing call).
    Alerting(Key),
    /// The call is connected.
    Connected(Key),
    /// The call is over.
    Ended(Key, End),
    /// Availability changed.
    Status(Status),
}

/// One way of calling.
pub trait Line: Send {
    /// True when this line carries calls of `way`.
    fn serves(&self, way: Way) -> bool;
    /// Starts a call; returns the key of the new call.
    fn dial(&mut self, way: Way, number: &str, video: bool) -> Result<Key, LineError>;
    /// Accepts a ringing call.
    fn answer(&mut self, key: Key) -> Result<(), LineError>;
    /// Ends or declines a call.
    fn release(&mut self, key: Key) -> Result<(), LineError>;
    /// Holds (`true`) or resumes a call.
    fn hold(&mut self, key: Key, on: bool) -> Result<(), LineError>;
    /// Sends one tone (`0-9`, `*`, `#`).
    fn dtmf(&mut self, key: Key, tone: u8) -> Result<(), LineError>;
    /// Applies the wanted media (camera, camera index, screen sharing).
    fn media(&mut self, key: Key, media: Media) -> Result<(), LineError>;
    /// Sends a short text to a number.
    fn send_text(&mut self, way: Way, number: &str, text: &str) -> Result<(), LineError>;
    /// Number of cameras usable for video calls.
    fn cameras(&self) -> u8;
    /// Pending events (non-blocking).
    fn poll(&mut self) -> Vec<LineEvent>;
}

/// Microphone and audio output of calls.
pub trait AudioRoute: Send {
    /// Mutes or unmutes the microphone.
    fn set_mute(&mut self, on: bool) -> Result<(), LineError>;
    /// Chooses the output.
    fn set_output(&mut self, output: Output) -> Result<(), LineError>;
    /// Outputs that exist now.
    fn outputs(&self) -> Vec<Output>;
}

/// Time source of the session (injected; tests use a manual clock).
pub trait Clock: Send {
    /// Monotonic milliseconds.
    fn now_ms(&self) -> u64;
    /// Wall clock, seconds since the Unix epoch (call log).
    fn wall_s(&self) -> i64;
}

/// The system clock.
#[derive(Debug)]
pub struct SystemClock {
    start: std::time::Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        SystemClock {
            start: std::time::Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn wall_s(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_display_and_system_clock_moves_forward() {
        assert_eq!(
            LineError::Unsupported.to_string(),
            "not supported on this way"
        );
        assert!(LineError::Unavailable(Failure::NoSim)
            .to_string()
            .contains("NoSim"));
        assert_eq!(LineError::UnknownCall.to_string(), "unknown call");
        assert_eq!(LineError::Backend.to_string(), "backend failure");
        let clock = SystemClock::default();
        let a = clock.now_ms();
        assert!(clock.now_ms() >= a);
        assert!(clock.wall_s() > 1_700_000_000);
    }
}
