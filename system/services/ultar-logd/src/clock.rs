//! Time as the log service sees it: injected, so that tests are exact.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The two clocks of the service.
pub trait Clock: Send + Sync {
    /// Wall-clock milliseconds since 1970-01-01 UTC (stamped on records).
    fn wall_ms(&self) -> u64;
    /// Monotonic milliseconds (rate limits, waits); never goes back.
    fn mono_ms(&self) -> u64;
}

/// The clocks of the machine.
#[derive(Debug)]
pub struct SystemClock(Instant);

impl SystemClock {
    /// Starts the monotonic clock at zero now.
    pub fn new() -> Self {
        Self(Instant::now())
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn wall_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            })
    }

    fn mono_ms(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// A clock that only moves when told to. Both clocks move together.
#[derive(Debug, Default)]
pub struct ManualClock {
    wall: AtomicU64,
    mono: AtomicU64,
}

impl ManualClock {
    /// A clock showing wall time `wall_ms` and monotonic time 0.
    pub fn at(wall_ms: u64) -> Self {
        Self {
            wall: AtomicU64::new(wall_ms),
            mono: AtomicU64::new(0),
        }
    }

    /// Moves both clocks forward.
    pub fn advance(&self, ms: u64) {
        self.wall.fetch_add(ms, Ordering::SeqCst);
        self.mono.fetch_add(ms, Ordering::SeqCst);
    }

    /// Sets the wall clock only (a user changing the date).
    pub fn set_wall(&self, wall_ms: u64) {
        self.wall.store(wall_ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn wall_ms(&self) -> u64 {
        self.wall.load(Ordering::SeqCst)
    }

    fn mono_ms(&self) -> u64 {
        self.mono.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manual_clock_moves_only_when_told() {
        let clock = ManualClock::at(1_000);
        assert_eq!((clock.wall_ms(), clock.mono_ms()), (1_000, 0));
        clock.advance(250);
        assert_eq!((clock.wall_ms(), clock.mono_ms()), (1_250, 250));
        clock.set_wall(5);
        assert_eq!((clock.wall_ms(), clock.mono_ms()), (5, 250));
    }

    #[test]
    fn the_system_clock_is_after_2024_and_monotonic() {
        let clock = SystemClock::default();
        assert!(clock.wall_ms() > 1_704_067_200_000);
        let first = clock.mono_ms();
        assert!(clock.mono_ms() >= first);
    }
}
