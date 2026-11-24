//! Wall-clock time for the back-off (stored across restarts), injectable for tests.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch.
pub trait Clock: Send + Sync {
    /// Now.
    fn now_ms(&self) -> u64;
}

/// The system clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
}

/// A clock that moves only when told (tests).
#[derive(Debug, Default)]
pub struct ManualClock(AtomicU64);

impl ManualClock {
    /// A clock at `ms`.
    pub fn new(ms: u64) -> ManualClock {
        ManualClock(AtomicU64::new(ms))
    }

    /// Moves the clock forward.
    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }

    /// Sets the clock (also backwards: a wrong system clock).
    pub fn set(&self, ms: u64) {
        self.0.store(ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clocks_tell_time() {
        assert!(SystemClock.now_ms() > 1_700_000_000_000);
        let c = ManualClock::new(5);
        c.advance(10);
        assert_eq!(c.now_ms(), 15);
        c.set(1);
        assert_eq!(c.now_ms(), 1);
    }
}
