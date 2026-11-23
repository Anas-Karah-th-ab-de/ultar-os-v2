//! Time for expiry checks on devices whose clock cannot be trusted.
//!
//! "Now" is the larger of the system clock and a stored **time floor**. The floor is raised only
//! from signed evidence (the `created` time of a manifest whose signature verified), never from
//! the local clock, and never goes backwards. A device without a battery-backed clock therefore
//! still refuses a release that expired before the newest release it has ever seen, and a clock
//! that was set far into the future cannot poison the stored value.

use crate::error::UpdateError;
use crate::monotonic::MonotonicStore;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Source of the current time in seconds since 1970 (UTC).
pub trait Clock: Send + Sync {
    /// Seconds since the Unix epoch; `0` when the clock is before it.
    fn now_unix(&self) -> u64;
}

/// The system's real-time clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

/// A clock that is set by hand (tests, simulations).
#[derive(Debug, Default)]
pub struct ManualClock(AtomicU64);

impl ManualClock {
    /// A clock that shows `now`.
    pub fn new(now: u64) -> Self {
        Self(AtomicU64::new(now))
    }

    /// Sets the time (also backwards).
    pub fn set(&self, now: u64) {
        self.0.store(now, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_unix(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// The time floor stored on the device.
#[derive(Debug, Clone)]
pub struct TimeFloor {
    store: MonotonicStore,
}

impl TimeFloor {
    /// The floor kept in `state_dir`.
    pub fn new(state_dir: &Path) -> Self {
        Self {
            store: MonotonicStore::new(state_dir, "time"),
        }
    }

    /// The stored floor (0 on a new device).
    pub fn get(&self) -> Result<u64, UpdateError> {
        self.store.get()
    }

    /// Records that the time `evidence` has certainly been reached.
    pub fn witness(&self, evidence: u64) -> Result<u64, UpdateError> {
        self.store.raise(evidence)
    }

    /// The time to judge expiry with: the clock, but never before the floor.
    pub fn effective_now(&self, clock: &dyn Clock) -> Result<u64, UpdateError> {
        Ok(clock.now_unix().max(self.get()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_bounds_a_clock_that_is_behind() {
        let dir = tempfile::tempdir().unwrap();
        let floor = TimeFloor::new(dir.path());
        let clock = ManualClock::new(0);
        assert_eq!(floor.effective_now(&clock).unwrap(), 0);
        floor.witness(1_800_000_000).unwrap();
        assert_eq!(floor.effective_now(&clock).unwrap(), 1_800_000_000);
        floor.witness(1_700_000_000).unwrap();
        assert_eq!(floor.get().unwrap(), 1_800_000_000, "never backwards");
        clock.set(1_900_000_000);
        assert_eq!(floor.effective_now(&clock).unwrap(), 1_900_000_000);
        assert_eq!(
            floor.get().unwrap(),
            1_800_000_000,
            "the local clock does not feed the floor"
        );
    }

    #[test]
    fn system_clock_is_after_2020() {
        assert!(SystemClock.now_unix() > 1_577_836_800);
    }
}
