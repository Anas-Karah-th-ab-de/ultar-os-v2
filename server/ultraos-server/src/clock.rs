//! Injectable monotonic clock, so rate limiting is testable without sleeping.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// A monotonic time source in milliseconds since an arbitrary origin.
pub trait Clock: Send + Sync + 'static {
    /// Milliseconds elapsed since the clock's origin. Never decreases.
    fn now_millis(&self) -> u64;
}

/// The real clock: time since process start (monotonic, unaffected by wall-clock changes).
#[derive(Debug)]
pub struct SystemClock {
    start: Instant,
}

impl SystemClock {
    /// Creates a clock whose origin is "now".
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// A clock that only moves when told to. For tests and simulations.
#[derive(Debug, Default)]
pub struct ManualClock {
    now: AtomicU64,
}

impl ManualClock {
    /// Creates a manual clock at the given time.
    pub fn new(start_millis: u64) -> Self {
        Self {
            now: AtomicU64::new(start_millis),
        }
    }

    /// Moves the clock forward (saturating).
    pub fn advance_millis(&self, millis: u64) {
        let mut cur = self.now.load(Ordering::SeqCst);
        loop {
            let next = cur.saturating_add(millis);
            match self
                .now
                .compare_exchange(cur, next, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return,
                Err(seen) => cur = seen,
            }
        }
    }
}

impl Clock for ManualClock {
    fn now_millis(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_moves_only_when_advanced() {
        let c = ManualClock::new(5);
        assert_eq!(c.now_millis(), 5);
        c.advance_millis(10);
        assert_eq!(c.now_millis(), 15);
        c.advance_millis(u64::MAX);
        assert_eq!(c.now_millis(), u64::MAX);
    }

    #[test]
    fn system_clock_is_monotonic() {
        let c = SystemClock::default();
        let a = c.now_millis();
        let b = c.now_millis();
        assert!(b >= a);
    }
}
