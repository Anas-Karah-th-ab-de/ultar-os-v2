//! Time source of the loop and measured composition times.

use std::time::Instant;

/// Milliseconds since some fixed point.
pub trait Clock {
    /// Now.
    fn now_ms(&self) -> u64;
}

/// The monotonic clock of the process.
#[derive(Debug)]
pub struct MonotonicClock(Instant);

impl Default for MonotonicClock {
    fn default() -> Self {
        MonotonicClock(Instant::now())
    }
}

impl Clock for MonotonicClock {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }
}

/// Measured composition times.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Frames composed.
    pub frames: u64,
    /// Longest composition in microseconds.
    pub max_compose_us: u64,
    /// Sum of composition times in microseconds.
    pub total_compose_us: u64,
}
