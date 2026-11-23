//! Per-client token-bucket rate limiting, in memory, bounded, with an injectable clock.
//!
//! * One bucket per (client key, limit class). IPv4 clients are keyed by address, IPv6 clients by
//!   their /64 prefix (one subscriber usually controls a whole /64).
//! * The table is bounded by [`LimiterConfig::max_entries`]. Buckets that have refilled completely
//!   carry no information and are evicted; this also means a client key is forgotten at the latest
//!   one refill window plus one sweep period after its last request.
//! * When the table is full of live buckets, unknown clients share one overflow bucket per class
//!   instead of evicting live entries (an attacker with many addresses cannot reset its own
//!   buckets or grow memory).
//!
//! Token arithmetic is exact integer math: one token is [`UNITS_PER_TOKEN`] units and a bucket
//! gains `refill_per_minute` units per millisecond.

use crate::clock::Clock;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard};

/// Internal units per token (milliseconds per minute), which makes refill exact.
const UNITS_PER_TOKEN: u64 = 60_000;
/// Minimum time between two automatic sweeps triggered by time.
const SWEEP_PERIOD_MS: u64 = 10_000;
/// Minimum time between two sweeps triggered by a full table.
const FULL_SWEEP_PERIOD_MS: u64 = 1_000;

/// Which budget a request is charged to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LimitClass {
    /// Catalogue, metadata and all other endpoints.
    General,
    /// File downloads (packages, system images).
    Download,
    /// Account registration and login (password hashing is expensive).
    Auth,
}

impl LimitClass {
    fn index(self) -> usize {
        match self {
            LimitClass::General => 0,
            LimitClass::Download => 1,
            LimitClass::Auth => 2,
        }
    }
}

/// Size and refill speed of one bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BucketConfig {
    /// Burst size: requests allowed at once from a full bucket (at least 1).
    pub capacity: u32,
    /// Sustained rate: tokens added per minute (at least 1).
    pub refill_per_minute: u32,
}

impl BucketConfig {
    fn capacity_units(self) -> u64 {
        u64::from(self.capacity.max(1)) * UNITS_PER_TOKEN
    }

    fn rate(self) -> u64 {
        u64::from(self.refill_per_minute.max(1))
    }

    /// Milliseconds an empty bucket needs to refill completely (the "rate window").
    pub fn window_millis(self) -> u64 {
        self.capacity_units().div_ceil(self.rate())
    }
}

/// Limits for all classes plus the table bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimiterConfig {
    /// Budget for [`LimitClass::General`].
    pub general: BucketConfig,
    /// Budget for [`LimitClass::Download`].
    pub download: BucketConfig,
    /// Budget for [`LimitClass::Auth`].
    pub auth: BucketConfig,
    /// Maximum number of buckets kept in memory.
    pub max_entries: usize,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        Self {
            general: BucketConfig {
                capacity: 120,
                refill_per_minute: 120,
            },
            download: BucketConfig {
                capacity: 30,
                refill_per_minute: 20,
            },
            auth: BucketConfig {
                capacity: 10,
                refill_per_minute: 6,
            },
            max_entries: 50_000,
        }
    }
}

impl LimiterConfig {
    fn bucket(&self, class: LimitClass) -> BucketConfig {
        match class {
            LimitClass::General => self.general,
            LimitClass::Download => self.download,
            LimitClass::Auth => self.auth,
        }
    }
}

/// Outcome of one rate-limit check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The request may proceed; one token was taken.
    Allowed,
    /// The request must be rejected.
    Limited {
        /// Seconds until one token is available again (at least 1).
        retry_after_secs: u64,
    },
}

/// Client key: IPv4 address or IPv6 /64 prefix (also used by [`crate::pwguard`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ClientKey {
    V4([u8; 4]),
    V6Prefix([u8; 8]),
}

impl ClientKey {
    pub(crate) fn from_ip(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(a) => ClientKey::V4(a.octets()),
            IpAddr::V6(a) => match a.to_ipv4_mapped() {
                Some(v4) => ClientKey::V4(v4.octets()),
                None => {
                    let o = a.octets();
                    ClientKey::V6Prefix([o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7]])
                }
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    units: u64,
    updated_ms: u64,
}

impl Bucket {
    fn full(cfg: BucketConfig, now: u64) -> Self {
        Self {
            units: cfg.capacity_units(),
            updated_ms: now,
        }
    }

    fn refill(&mut self, cfg: BucketConfig, now: u64) {
        let elapsed = now.saturating_sub(self.updated_ms);
        let gained = elapsed.saturating_mul(cfg.rate());
        self.units = self.units.saturating_add(gained).min(cfg.capacity_units());
        self.updated_ms = now;
    }

    fn is_full_at(&self, cfg: BucketConfig, now: u64) -> bool {
        let mut copy = *self;
        copy.refill(cfg, now);
        copy.units >= cfg.capacity_units()
    }

    fn take(&mut self, cfg: BucketConfig, now: u64) -> Decision {
        self.refill(cfg, now);
        if self.units >= UNITS_PER_TOKEN {
            self.units -= UNITS_PER_TOKEN;
            return Decision::Allowed;
        }
        let missing = UNITS_PER_TOKEN - self.units;
        let wait_ms = missing.div_ceil(cfg.rate());
        Decision::Limited {
            retry_after_secs: wait_ms.div_ceil(1000).max(1),
        }
    }
}

struct Inner {
    buckets: HashMap<(ClientKey, LimitClass), Bucket>,
    overflow: [Option<Bucket>; 3],
    last_sweep_ms: u64,
}

/// The rate limiter. Cheap to share behind an `Arc`.
pub struct RateLimiter {
    clock: Arc<dyn Clock>,
    cfg: LimiterConfig,
    inner: Mutex<Inner>,
}

impl RateLimiter {
    /// Creates a limiter with the given limits and time source.
    pub fn new(cfg: LimiterConfig, clock: Arc<dyn Clock>) -> Self {
        let now = clock.now_millis();
        let inner = Inner {
            buckets: HashMap::new(),
            overflow: [None; 3],
            last_sweep_ms: now,
        };
        Self {
            clock,
            cfg,
            inner: Mutex::new(inner),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Charges one request of `class` to the client at `ip`.
    pub fn check(&self, ip: IpAddr, class: LimitClass) -> Decision {
        let now = self.clock.now_millis();
        let cfg = self.cfg.bucket(class);
        let key = (ClientKey::from_ip(ip), class);
        let mut inner = self.lock();
        if now.saturating_sub(inner.last_sweep_ms) >= SWEEP_PERIOD_MS {
            Self::sweep_locked(&mut inner, &self.cfg, now);
        }
        if let Some(bucket) = inner.buckets.get_mut(&key) {
            return bucket.take(cfg, now);
        }
        let full = inner.buckets.len() >= self.cfg.max_entries;
        if full && now.saturating_sub(inner.last_sweep_ms) >= FULL_SWEEP_PERIOD_MS {
            Self::sweep_locked(&mut inner, &self.cfg, now);
        }
        if inner.buckets.len() >= self.cfg.max_entries {
            let shared = inner.overflow[class.index()].get_or_insert(Bucket::full(cfg, now));
            return shared.take(cfg, now);
        }
        let mut bucket = Bucket::full(cfg, now);
        let decision = bucket.take(cfg, now);
        inner.buckets.insert(key, bucket);
        decision
    }

    fn sweep_locked(inner: &mut Inner, cfg: &LimiterConfig, now: u64) -> usize {
        let before = inner.buckets.len();
        inner
            .buckets
            .retain(|(_, class), b| !b.is_full_at(cfg.bucket(*class), now));
        inner.last_sweep_ms = now;
        before - inner.buckets.len()
    }

    /// Evicts every bucket that has refilled completely. Returns how many were removed.
    ///
    /// Called automatically from [`RateLimiter::check`]; the server also calls it from a timer so
    /// client keys are dropped even when no further request arrives.
    pub fn sweep(&self) -> usize {
        let now = self.clock.now_millis();
        let mut inner = self.lock();
        Self::sweep_locked(&mut inner, &self.cfg, now)
    }

    /// Number of client buckets currently held.
    pub fn len(&self) -> usize {
        self.lock().buckets.len()
    }

    /// True when no client bucket is held.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The configured limits.
    pub fn config(&self) -> LimiterConfig {
        self.cfg
    }
}

#[cfg(test)]
#[path = "ratelimit_tests.rs"]
mod tests;
