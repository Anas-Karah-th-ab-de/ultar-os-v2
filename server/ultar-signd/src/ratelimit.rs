//! Token bucket per key role, with an injected clock (seconds) so tests are deterministic.

use crate::keys::Role;

/// Limits of one role: at most `burst` signatures at once, refilled at `per_hour`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    /// Bucket size.
    pub burst: u32,
    /// Refill per hour.
    pub per_hour: u32,
}

impl Limit {
    /// The default limit of a role: releases are rare, store publishing signs every app.
    pub const fn default_for(role: Role) -> Limit {
        match role {
            Role::Os => Limit {
                burst: 10,
                per_hour: 20,
            },
            Role::System => Limit {
                burst: 60,
                per_hour: 120,
            },
            Role::Store => Limit {
                burst: 200,
                per_hour: 600,
            },
        }
    }
}

#[derive(Debug, Clone)]
struct Bucket {
    limit: Limit,
    /// Tokens times 3600 (integer arithmetic, no float drift).
    milli: u64,
    last: u64,
}

/// One bucket per role.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    buckets: [Bucket; 3],
}

fn index(role: Role) -> usize {
    match role {
        Role::Os => 0,
        Role::System => 1,
        Role::Store => 2,
    }
}

impl RateLimiter {
    /// Full buckets with the given limits (`limits[i]` for `Role::ALL[i]`) at time `now`.
    pub fn new(limits: [Limit; 3], now: u64) -> RateLimiter {
        let b = |l: Limit| Bucket {
            limit: l,
            milli: u64::from(l.burst) * 3600,
            last: now,
        };
        RateLimiter {
            buckets: [b(limits[0]), b(limits[1]), b(limits[2])],
        }
    }

    /// Takes one token of `role` at time `now` (seconds); false when the bucket is empty.
    /// A clock that goes backwards refills nothing.
    pub fn take(&mut self, role: Role, now: u64) -> bool {
        let b = &mut self.buckets[index(role)];
        let elapsed = now.saturating_sub(b.last);
        b.last = b.last.max(now);
        let cap = u64::from(b.limit.burst) * 3600;
        b.milli = cap.min(
            b.milli
                .saturating_add(elapsed.saturating_mul(u64::from(b.limit.per_hour))),
        );
        if b.milli >= 3600 {
            b.milli -= 3600;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_then_refill_per_role() {
        let lim = Limit {
            burst: 2,
            per_hour: 3600,
        };
        let mut r = RateLimiter::new([lim, Limit::default_for(Role::System), lim], 100);
        assert!(r.take(Role::Os, 100));
        assert!(r.take(Role::Os, 100));
        assert!(!r.take(Role::Os, 100));
        // The other roles are not affected.
        assert!(r.take(Role::Store, 100));
        // One token per second at 3600/h.
        assert!(r.take(Role::Os, 101));
        assert!(!r.take(Role::Os, 101));
        // Clock backwards: nothing refilled.
        assert!(!r.take(Role::Os, 50));
        // Long pause: capped at the burst.
        assert!(r.take(Role::Os, 10_000));
        assert!(r.take(Role::Os, 10_000));
        assert!(!r.take(Role::Os, 10_000));
    }

    #[test]
    fn defaults_are_strictest_for_releases() {
        let os = Limit::default_for(Role::Os);
        let store = Limit::default_for(Role::Store);
        assert!(os.burst < store.burst && os.per_hour < store.per_hour);
        let mut r = RateLimiter::new(Role::ALL.map(Limit::default_for), 0);
        let granted = (0..100).filter(|_| r.take(Role::Os, 0)).count();
        assert_eq!(granted, 10);
    }
}
