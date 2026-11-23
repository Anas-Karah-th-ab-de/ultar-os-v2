//! Budget of failed password checks per account.
//!
//! The password is checked by the login, the password change and the start of a lost-device
//! recovery. All three draw on one budget per account: after [`DEFAULT_MAX_FAILURES`] failed
//! checks within [`WINDOW_MS`] every further check for that account is refused until the window
//! is over, whatever address or token the requests come from. Device keys are not affected: a
//! locked budget stops password use, not the account's devices.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

/// Failed checks allowed per account and window when not configured.
pub const DEFAULT_MAX_FAILURES: u32 = 10;
/// Length of the window in milliseconds (15 minutes).
pub const WINDOW_MS: u64 = 15 * 60 * 1000;
/// Accounts tracked at the same time; beyond that, expired entries are dropped first and new
/// accounts are refused (fail closed) until room is free.
pub const MAX_TRACKED: usize = 100_000;

struct Entry {
    attempts: u32,
    since_ms: u64,
}

/// The table of running budgets.
pub struct PasswordGuard {
    inner: Mutex<HashMap<i64, Entry>>,
    max_failures: u32,
    window_ms: u64,
    max_tracked: usize,
}

impl PasswordGuard {
    /// A guard that allows `max_failures` failed checks per account and window.
    pub fn new(max_failures: u32) -> Self {
        Self::with_bounds(max_failures, WINDOW_MS, MAX_TRACKED)
    }

    /// A guard with explicit window and table size.
    pub fn with_bounds(max_failures: u32, window_ms: u64, max_tracked: usize) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            max_failures: max_failures.max(1),
            window_ms,
            max_tracked,
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<i64, Entry>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn wait_secs(&self, since_ms: u64, now_ms: u64) -> u64 {
        self.window_ms
            .saturating_sub(now_ms.saturating_sub(since_ms))
            .div_ceil(1000)
            .max(1)
    }

    /// Books one password check for `account_id` before the password is looked at. `Err` carries
    /// the seconds until the next check is allowed. A check that turns out right gives its place
    /// back with [`PasswordGuard::succeeded`]; booking first keeps parallel guesses inside the
    /// budget.
    pub fn attempt(&self, account_id: i64, now_ms: u64) -> Result<(), u64> {
        let mut table = self.lock();
        let window = self.window_ms;
        let live = |e: &Entry| now_ms.saturating_sub(e.since_ms) < window;
        if let Some(entry) = table.get_mut(&account_id).filter(|e| live(e)) {
            if entry.attempts >= self.max_failures {
                return Err(self.wait_secs(entry.since_ms, now_ms));
            }
            entry.attempts += 1;
            return Ok(());
        }
        if table.len() >= self.max_tracked {
            table.retain(|_, e| live(e));
            if table.len() >= self.max_tracked {
                return Err(self.wait_secs(now_ms, now_ms));
            }
        }
        let fresh = Entry {
            attempts: 1,
            since_ms: now_ms,
        };
        table.insert(account_id, fresh);
        Ok(())
    }

    /// The booked check was right: it does not count.
    pub fn succeeded(&self, account_id: i64) {
        let mut table = self.lock();
        if let Some(entry) = table.get_mut(&account_id) {
            entry.attempts = entry.attempts.saturating_sub(1);
            if entry.attempts == 0 {
                table.remove(&account_id);
            }
        }
    }

    /// Number of accounts with a running budget.
    pub fn tracked(&self) -> usize {
        self.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_are_counted_per_account_and_window() {
        let guard = PasswordGuard::with_bounds(3, 60_000, 10);
        for _ in 0..3 {
            assert_eq!(guard.attempt(1, 0), Ok(()));
        }
        assert_eq!(guard.attempt(1, 30_000), Err(30));
        assert_eq!(guard.attempt(1, 59_999), Err(1));
        assert_eq!(guard.attempt(2, 30_000), Ok(()), "another account");
        assert_eq!(guard.attempt(1, 60_000), Ok(()), "new window");
        assert_eq!(guard.tracked(), 2);
    }

    #[test]
    fn right_passwords_do_not_use_up_the_budget() {
        let guard = PasswordGuard::with_bounds(2, 60_000, 10);
        for _ in 0..20 {
            assert_eq!(guard.attempt(1, 0), Ok(()));
            guard.succeeded(1);
        }
        assert_eq!(guard.tracked(), 0);
        assert_eq!(guard.attempt(1, 0), Ok(()));
        assert_eq!(guard.attempt(1, 0), Ok(()));
        guard.succeeded(1);
        assert_eq!(
            guard.attempt(1, 0),
            Ok(()),
            "one failure and one place left"
        );
        assert!(guard.attempt(1, 0).is_err());
        guard.succeeded(99); // unknown account: nothing happens
    }

    #[test]
    fn parallel_guesses_cannot_exceed_the_budget() {
        let guard = std::sync::Arc::new(PasswordGuard::with_bounds(5, 60_000, 10));
        let threads: Vec<_> = (0..16)
            .map(|_| {
                let guard = guard.clone();
                std::thread::spawn(move || guard.attempt(1, 0).is_ok())
            })
            .collect();
        let booked = threads
            .into_iter()
            .filter_map(|t| t.join().ok())
            .filter(|ok| *ok)
            .count();
        assert_eq!(booked, 5);
    }

    #[test]
    fn the_table_is_bounded_and_fails_closed() {
        let guard = PasswordGuard::with_bounds(3, 1000, 2);
        assert!(guard.attempt(1, 0).is_ok());
        assert!(guard.attempt(2, 0).is_ok());
        assert_eq!(guard.attempt(3, 500), Err(1), "full: refused");
        assert!(guard.attempt(3, 1000).is_ok(), "expired entries make room");
        assert_eq!(guard.tracked(), 1);
        assert_eq!(PasswordGuard::new(0).max_failures, 1);
    }
}
