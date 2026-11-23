//! Rate limit for log records: a token bucket with an injected clock.

/// A source of monotonic time in milliseconds.
pub trait Clock: Send + Sync {
    /// Milliseconds since an arbitrary fixed point; never goes back.
    fn now_ms(&self) -> u64;
}

/// The monotonic clock of the machine.
#[derive(Debug)]
pub struct MonotonicClock(std::time::Instant);

impl MonotonicClock {
    /// A clock that starts at zero now.
    pub fn new() -> Self {
        Self(std::time::Instant::now())
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MonotonicClock {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// Parameters of a token bucket.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RateLimit {
    /// Records that may pass at once after a quiet time.
    pub burst: u32,
    /// Records per second in the long run.
    pub per_second: u32,
}

impl RateLimit {
    /// 200 at once, 50 per second.
    pub const STANDARD: RateLimit = RateLimit {
        burst: 200,
        per_second: 50,
    };
}

/// One token costs this many units; units refill at `per_second` per ms.
const UNITS_PER_TOKEN: u64 = 1000;

/// A token bucket. Integer arithmetic only: the same sequence of calls and
/// clock values always gives the same decisions.
#[derive(Clone, Debug)]
pub struct TokenBucket {
    limit: RateLimit,
    units: u64,
    last_ms: u64,
    dropped: u64,
}

impl TokenBucket {
    /// A full bucket at time `now_ms`.
    pub fn new(limit: RateLimit, now_ms: u64) -> Self {
        Self {
            limit,
            units: u64::from(limit.burst) * UNITS_PER_TOKEN,
            last_ms: now_ms,
            dropped: 0,
        }
    }

    fn refill(&mut self, now_ms: u64) {
        let elapsed = now_ms.saturating_sub(self.last_ms);
        self.last_ms = self.last_ms.max(now_ms);
        let capacity = u64::from(self.limit.burst) * UNITS_PER_TOKEN;
        let gained = elapsed.saturating_mul(u64::from(self.limit.per_second));
        self.units = self.units.saturating_add(gained).min(capacity);
    }

    /// Whether one record may pass at `now_ms`. A refused record is
    /// counted; see [`Self::take_dropped`].
    pub fn allow(&mut self, now_ms: u64) -> bool {
        self.refill(now_ms);
        if self.units >= UNITS_PER_TOKEN {
            self.units -= UNITS_PER_TOKEN;
            true
        } else {
            self.dropped = self.dropped.saturating_add(1);
            false
        }
    }

    /// Records refused since the last call; resets the counter. The caller
    /// writes a "N records dropped" marker with it.
    pub fn take_dropped(&mut self) -> u64 {
        std::mem::take(&mut self.dropped)
    }

    /// Records refused and not yet reported.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_then_steady_rate() {
        let limit = RateLimit {
            burst: 5,
            per_second: 10,
        };
        let mut bucket = TokenBucket::new(limit, 1_000);
        let passed = (0..100).filter(|_| bucket.allow(1_000)).count();
        assert_eq!(passed, 5);
        assert_eq!(bucket.dropped(), 95);
        // 10 per second = one per 100 ms.
        assert!(!bucket.allow(1_099));
        assert!(bucket.allow(1_100));
        assert!(!bucket.allow(1_100));
        // One second later: ten more, never more than the burst.
        let passed = (0..100).filter(|_| bucket.allow(2_100)).count();
        assert_eq!(passed, 5);
        assert_eq!(bucket.take_dropped(), 95 + 2 + 95);
        assert_eq!(bucket.take_dropped(), 0);
    }

    #[test]
    fn a_long_flood_passes_exactly_burst_plus_rate_times_time() {
        let limit = RateLimit {
            burst: 200,
            per_second: 50,
        };
        let mut bucket = TokenBucket::new(limit, 0);
        // 1 000 000 records in 10 s (100 per ms).
        let mut passed = 0u64;
        for index in 0..1_000_000u64 {
            if bucket.allow(index / 100) {
                passed += 1;
            }
        }
        assert_eq!(passed, 200 + 50 * 10 - 1);
        assert_eq!(bucket.dropped(), 1_000_000 - passed);
    }

    #[test]
    fn a_clock_that_stands_still_or_jumps_does_no_harm() {
        let mut bucket = TokenBucket::new(RateLimit::STANDARD, 500);
        // Time "before" the start is treated as no time passed.
        for _ in 0..200 {
            assert!(bucket.allow(0));
        }
        assert!(!bucket.allow(0));
        // A huge jump cannot overflow and cannot exceed the burst.
        let passed = (0..1_000).filter(|_| bucket.allow(u64::MAX)).count();
        assert_eq!(passed, 200);
        let zero = RateLimit {
            burst: 0,
            per_second: 0,
        };
        let mut closed = TokenBucket::new(zero, 0);
        assert!(!closed.allow(u64::MAX));
    }

    #[test]
    fn the_real_clock_moves_forward() {
        let clock = MonotonicClock::default();
        let first = clock.now_ms();
        assert!(clock.now_ms() >= first);
    }
}
