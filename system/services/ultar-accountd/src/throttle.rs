//! Limits: how often one caller may ask for changes, and how long a user
//! waits after a wrong current password (on top of the authentication
//! service's own waits). Times are milliseconds of an injected clock.

use std::collections::BTreeMap;

/// Callers remembered at once (the oldest is forgotten).
pub const MAX_CALLERS: usize = 64;
/// Changes one caller may ask for at once.
pub const BURST: u32 = 8;
/// A new change is allowed every this many milliseconds.
pub const REFILL_MS: u64 = 2_000;
/// Longest wait after wrong passwords.
pub const MAX_WAIT_MS: u64 = 300_000;

/// A token bucket per caller.
#[derive(Debug, Default)]
pub struct Limiter {
    buckets: BTreeMap<String, (u32, u64)>,
}

impl Limiter {
    /// True when `caller` may make one more change now.
    pub fn allow(&mut self, caller: &str, now: u64) -> bool {
        if !self.buckets.contains_key(caller) && self.buckets.len() >= MAX_CALLERS {
            let oldest = self
                .buckets
                .iter()
                .min_by_key(|(_, (_, t))| *t)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                self.buckets.remove(&k);
            }
        }
        let (tokens, last) = self
            .buckets
            .entry(caller.to_owned())
            .or_insert((BURST, now));
        let earned = now.saturating_sub(*last) / REFILL_MS;
        if earned > 0 {
            *tokens = (*tokens)
                .saturating_add(earned.min(u64::from(BURST)) as u32)
                .min(BURST);
            *last += earned * REFILL_MS;
        }
        if *tokens == 0 {
            return false;
        }
        *tokens -= 1;
        true
    }
}

/// Waits after wrong current passwords, per account: 1 s · 2ⁿ up to 5 min.
#[derive(Debug, Default)]
pub struct Failures {
    by_user: BTreeMap<String, (u32, u64)>,
}

impl Failures {
    /// Milliseconds the user still has to wait.
    pub fn remaining(&self, user: &str, now: u64) -> u64 {
        self.by_user
            .get(user)
            .map_or(0, |(_, until)| until.saturating_sub(now))
    }

    /// Counts a wrong password; returns the wait.
    pub fn fail(&mut self, user: &str, now: u64) -> u64 {
        if !self.by_user.contains_key(user) && self.by_user.len() >= MAX_CALLERS {
            return MAX_WAIT_MS;
        }
        let entry = self.by_user.entry(user.to_owned()).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(1);
        let wait = (1_000u64 << entry.0.saturating_sub(1).min(20)).min(MAX_WAIT_MS);
        entry.1 = now + wait;
        wait
    }

    /// The right password: the count starts again.
    pub fn succeed(&mut self, user: &str) {
        self.by_user.remove(user);
    }

    /// An account is gone.
    pub fn forget(&mut self, user: &str) {
        self.by_user.remove(user);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caller_gets_a_burst_then_one_change_per_refill() {
        let mut l = Limiter::default();
        for _ in 0..BURST {
            assert!(l.allow("app.settings", 0));
        }
        assert!(!l.allow("app.settings", 0));
        assert!(l.allow("app.other", 0), "others are not affected");
        assert!(!l.allow("app.settings", REFILL_MS - 1));
        assert!(l.allow("app.settings", REFILL_MS));
        assert!(!l.allow("app.settings", REFILL_MS));
        for i in 0..200 {
            l.allow(&format!("app.x{i}"), 10);
        }
        assert!(l.buckets.len() <= MAX_CALLERS);
    }

    #[test]
    fn waits_grow_and_end() {
        let mut f = Failures::default();
        assert_eq!(f.fail("leila", 0), 1_000);
        assert_eq!(f.fail("leila", 0), 2_000);
        assert_eq!(f.remaining("leila", 500), 1_500);
        for _ in 0..30 {
            f.fail("leila", 0);
        }
        assert_eq!(f.remaining("leila", 0), MAX_WAIT_MS);
        f.succeed("leila");
        assert_eq!(f.remaining("leila", 0), 0);
        for i in 0..MAX_CALLERS {
            f.fail(&format!("u{i}"), 0);
        }
        assert_eq!(f.fail("one-too-many", 0), MAX_WAIT_MS);
        f.forget("u1");
        assert_eq!(f.remaining("u1", 0), 0);
    }
}
