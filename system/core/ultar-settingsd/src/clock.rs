//! Time as the service sees it: injected, so every deadline is testable.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// A monotonic clock.
pub trait Clock: Send + Sync {
    /// Time since an arbitrary fixed start; never goes backwards.
    fn now(&self) -> Duration;

    /// How long a real thread may block before `deadline` is reached.
    ///
    /// `None` means "this clock does not advance by itself": the owner of a
    /// manual clock wakes the service explicitly after moving the time.
    fn real_wait(&self, deadline: Duration) -> Option<Duration>;
}

/// The machine's monotonic clock.
#[derive(Debug, Clone)]
pub struct SystemClock {
    start: Instant,
}

impl SystemClock {
    /// A clock that starts counting now.
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
    fn now(&self) -> Duration {
        self.start.elapsed()
    }

    fn real_wait(&self, deadline: Duration) -> Option<Duration> {
        Some(deadline.saturating_sub(self.now()))
    }
}

/// A clock that only moves when told to (tests, simulations).
#[derive(Debug, Clone, Default)]
pub struct ManualClock {
    now: Arc<Mutex<Duration>>,
}

impl ManualClock {
    /// A clock standing at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves the time forward.
    pub fn advance(&self, by: Duration) {
        let mut now = self.now.lock().unwrap_or_else(PoisonError::into_inner);
        *now = now.saturating_add(by);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn real_wait(&self, _deadline: Duration) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_moves_only_on_request() {
        let clock = ManualClock::new();
        let other = clock.clone();
        assert_eq!(clock.now(), Duration::ZERO);
        other.advance(Duration::from_secs(3));
        assert_eq!(clock.now(), Duration::from_secs(3));
        assert_eq!(clock.real_wait(Duration::from_secs(9)), None);
        clock.advance(Duration::MAX);
        assert_eq!(clock.now(), Duration::MAX);
    }

    #[test]
    fn system_clock_is_monotonic_and_bounded() {
        let clock = SystemClock::default();
        let first = clock.now();
        assert!(clock.now() >= first);
        assert_eq!(clock.real_wait(Duration::ZERO), Some(Duration::ZERO));
        let wait = clock.real_wait(first + Duration::from_secs(60));
        assert!(wait.is_some_and(|wait| wait <= Duration::from_secs(60)));
    }
}
