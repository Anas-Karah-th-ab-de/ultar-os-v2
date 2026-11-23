//! Budgets of failed password checks.
//!
//! The password is checked by the login, the password change and the start of a lost-device
//! recovery. Every check is booked before the password is looked at (so parallel guesses cannot
//! exceed a budget); a right password gives its place back. Two budgets apply:
//!
//! * **Per account and client** (IPv4 address or IPv6 /64): after `pair_max` failed checks
//!   within [`WINDOW_MS`] this client is refused for this account until the window is over. A
//!   stranger who guesses from his addresses locks out only those addresses.
//! * **Per account**, whatever the client: the first [`ACCOUNT_FREE`] failures cost nothing;
//!   after that every check waits, the wait growing with the failures up to [`MAX_DELAY`]. Only
//!   at [`ACCOUNT_MAX_FAILURES`] failures (many clients guessing together) are checks of the
//!   account refused. A check authenticated by a managing device of the account is exempt from
//!   this part: the owner with a device in hand can always change his password and stop a
//!   recovery.
//!
//! Device keys are not affected: a budget stops password use, not the account's devices.

use crate::ratelimit::ClientKey;
use std::collections::HashMap;
use std::hash::Hash;
use std::net::IpAddr;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

/// Failed checks allowed per account and client within the window when not configured.
pub const DEFAULT_MAX_FAILURES: u32 = 20;
/// Length of the window in milliseconds (15 minutes).
pub const WINDOW_MS: u64 = 15 * 60 * 1000;
/// Failures of an account (all clients) that cost no waiting.
pub const ACCOUNT_FREE: u32 = 10;
/// Failures of an account (all clients) after which its checks are refused – unless made with a
/// managing device's token.
pub const ACCOUNT_MAX_FAILURES: u32 = 200;
/// Longest wait before a check.
pub const MAX_DELAY: Duration = Duration::from_secs(8);
/// Entries kept per table. When a table is full, expired entries go first, then the entry whose
/// window started earliest: a full table never refuses a check.
pub const MAX_TRACKED: usize = 100_000;

#[derive(Debug, Clone, Copy)]
struct Entry {
    failures: u32,
    since_ms: u64,
}

/// What a booked check must do before it looks at the password, and what to give back if the
/// password is right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Booking {
    account_id: i64,
    client: ClientKey,
    account_booked: bool,
    /// Time to wait before the check.
    pub delay: Duration,
}

/// A bounded table of failure counts with fixed windows.
struct Table<K> {
    entries: HashMap<K, Entry>,
}

impl<K: Copy + Eq + Hash> Table<K> {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Failures of `key` in the running window.
    fn failures(&self, key: &K, now_ms: u64, window_ms: u64) -> u32 {
        self.entries
            .get(key)
            .filter(|e| now_ms.saturating_sub(e.since_ms) < window_ms)
            .map_or(0, |e| e.failures)
    }

    fn since(&self, key: &K) -> u64 {
        self.entries.get(key).map_or(0, |e| e.since_ms)
    }

    fn book(&mut self, key: K, now_ms: u64, window_ms: u64, max_tracked: usize) {
        let live = |e: &Entry| now_ms.saturating_sub(e.since_ms) < window_ms;
        if let Some(entry) = self.entries.get_mut(&key) {
            if live(entry) {
                entry.failures = entry.failures.saturating_add(1);
                return;
            }
        }
        if self.entries.len() >= max_tracked && !self.entries.contains_key(&key) {
            self.entries.retain(|_, e| live(e));
            if self.entries.len() >= max_tracked {
                let oldest = self.entries.iter().min_by_key(|(_, e)| e.since_ms);
                if let Some(oldest) = oldest.map(|(k, _)| *k) {
                    self.entries.remove(&oldest);
                }
            }
        }
        let fresh = Entry {
            failures: 1,
            since_ms: now_ms,
        };
        self.entries.insert(key, fresh);
    }

    fn give_back(&mut self, key: &K) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.failures = entry.failures.saturating_sub(1);
            if entry.failures == 0 {
                self.entries.remove(key);
            }
        }
    }
}

struct Inner {
    pairs: Table<(i64, ClientKey)>,
    accounts: Table<i64>,
}

/// The budgets of all accounts.
pub struct PasswordGuard {
    inner: Mutex<Inner>,
    pair_max: u32,
    window_ms: u64,
    max_tracked: usize,
}

/// The wait before a check of an account that already has `failures` failures.
pub fn delay_for(failures: u32) -> Duration {
    if failures < ACCOUNT_FREE {
        return Duration::ZERO;
    }
    let doublings = (failures - ACCOUNT_FREE).min(4);
    Duration::from_secs(1u64 << doublings).min(MAX_DELAY)
}

impl PasswordGuard {
    /// A guard that allows `pair_max` failed checks per account and client within the window.
    pub fn new(pair_max: u32) -> Self {
        Self::with_bounds(pair_max, WINDOW_MS, MAX_TRACKED)
    }

    /// A guard with explicit window and table size.
    pub fn with_bounds(pair_max: u32, window_ms: u64, max_tracked: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                pairs: Table::new(),
                accounts: Table::new(),
            }),
            pair_max: pair_max.max(1),
            window_ms,
            max_tracked: max_tracked.max(1),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
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

    /// Books one password check of `account_id` from `client`. `managing`: the request carries
    /// the token of a managing device of the account (exempt from the account-wide budget).
    /// `Err` carries the seconds until a check from this client is allowed again.
    pub fn book(
        &self,
        account_id: i64,
        client: IpAddr,
        managing: bool,
        now_ms: u64,
    ) -> Result<Booking, u64> {
        let client = ClientKey::from_ip(client);
        let pair = (account_id, client);
        let window = self.window_ms;
        let mut inner = self.lock();
        if inner.pairs.failures(&pair, now_ms, window) >= self.pair_max {
            return Err(self.wait_secs(inner.pairs.since(&pair), now_ms));
        }
        let account_failures = inner.accounts.failures(&account_id, now_ms, window);
        if !managing && account_failures >= ACCOUNT_MAX_FAILURES {
            return Err(self.wait_secs(inner.accounts.since(&account_id), now_ms));
        }
        inner.pairs.book(pair, now_ms, window, self.max_tracked);
        if !managing {
            inner
                .accounts
                .book(account_id, now_ms, window, self.max_tracked);
        }
        Ok(Booking {
            account_id,
            client,
            account_booked: !managing,
            delay: if managing {
                Duration::ZERO
            } else {
                delay_for(account_failures)
            },
        })
    }

    /// The booked check was right: it does not count.
    pub fn succeeded(&self, booking: &Booking) {
        let mut inner = self.lock();
        inner.pairs.give_back(&(booking.account_id, booking.client));
        if booking.account_booked {
            inner.accounts.give_back(&booking.account_id);
        }
    }

    /// Number of entries in the two tables (account and client pairs, accounts).
    pub fn tracked(&self) -> (usize, usize) {
        let inner = self.lock();
        (inner.pairs.entries.len(), inner.accounts.entries.len())
    }
}

#[cfg(test)]
#[path = "pwguard_tests.rs"]
mod tests;
