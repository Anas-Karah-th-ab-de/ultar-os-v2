//! Back-off after wrong secrets: per user, the wait doubles with every failure (1 s, 2 s,
//! 4 s … at most [`MAX_WAIT_MS`]), a success clears it. Stored in the file `throttle`
//! (`fail <key> <count> <until ms>` per line) so that a restart does not reset it.

use crate::file::{self, FileError};
use std::collections::BTreeMap;
use std::path::Path;

/// First wait after a failure.
pub const FIRST_WAIT_MS: u64 = 1_000;
/// Longest wait.
pub const MAX_WAIT_MS: u64 = 300_000;
/// Header word of the file.
pub const KIND: &str = "ultar-auth-throttle";
/// File name in the directory.
pub const FILE: &str = "throttle";
/// Key whose wait holds for every user (set when the counter file was damaged).
pub const EVERYONE: &str = "!";
/// Failures in a row after which a PIN is no longer checked: only the password (whose
/// success clears the counter) unlocks again (security review S-6: a 4-digit PIN cannot
/// be guessed through the back-off alone).
pub const PIN_LOCKOUT: u32 = 10;
/// Keys kept (users plus the shared key for unknown names).
pub const MAX_KEYS: usize = 128;

/// Failures of one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Entry {
    /// Failures in a row.
    pub failures: u32,
    /// No try is checked before this time (ms since the epoch).
    pub until_ms: u64,
}

/// The wait after `failures` failures in a row.
pub fn wait_after(failures: u32) -> u64 {
    if failures == 0 {
        return 0;
    }
    let doubling = failures.saturating_sub(1).min(20);
    FIRST_WAIT_MS
        .saturating_mul(1u64 << doubling)
        .min(MAX_WAIT_MS)
}

/// All counters.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Throttle {
    entries: BTreeMap<String, Entry>,
}

impl Throttle {
    /// Remaining wait for `key` at `now`, 0 when a try may be checked. A clock that went
    /// back keeps the stored wait at most [`MAX_WAIT_MS`] long.
    pub fn remaining(&self, key: &str, now: u64) -> u64 {
        self.entries
            .get(key)
            .map_or(0, |e| e.until_ms.saturating_sub(now).min(MAX_WAIT_MS))
    }

    /// Counts a failure; returns the wait before the next try.
    pub fn fail(&mut self, key: &str, now: u64) -> u64 {
        if !self.entries.contains_key(key) && self.entries.len() >= MAX_KEYS {
            // The oldest wait is dropped; failures of the shared key always count.
            if let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.until_ms)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest);
            }
        }
        let e = self.entries.entry(key.to_owned()).or_default();
        e.failures = e.failures.saturating_add(1);
        let wait = wait_after(e.failures);
        e.until_ms = now.saturating_add(wait);
        wait
    }

    /// Clears the counter after a success.
    pub fn succeed(&mut self, key: &str) {
        self.entries.remove(key);
    }

    /// Failures in a row of `key`.
    pub fn failures(&self, key: &str) -> u32 {
        self.entries.get(key).map_or(0, |e| e.failures)
    }

    /// The file body.
    pub fn encode(&self) -> String {
        self.entries
            .iter()
            .map(|(k, e)| format!("fail {k} {} {}\n", e.failures, e.until_ms))
            .collect()
    }

    /// Reads a file body.
    pub fn decode(body: &str) -> Result<Throttle, FileError> {
        let mut entries = BTreeMap::new();
        for line in body.lines() {
            let mut p = line.split(' ');
            let ok = p.next() == Some("fail");
            let key = p.next().filter(|k| !k.is_empty()).map(str::to_owned);
            let failures = p.next().and_then(|v| v.parse().ok());
            let until_ms = p.next().and_then(|v| v.parse().ok());
            match (ok, key, failures, until_ms, p.next()) {
                (true, Some(k), Some(failures), Some(until_ms), None)
                    if entries.len() < MAX_KEYS =>
                {
                    entries.insert(k, Entry { failures, until_ms });
                }
                _ => return Err(FileError::Damaged),
            }
        }
        Ok(Throttle { entries })
    }

    /// Loads the counters; a damaged or unreadable file counts as the longest wait for
    /// everybody, so that damaging it (or making it unreadable) cannot reset the back-off.
    pub fn load(dir: &Path, now: u64) -> Result<Throttle, FileError> {
        match file::read(&dir.join(FILE), KIND).and_then(|body| match body {
            Some(body) => Throttle::decode(&body).map(Some),
            None => Ok(None),
        }) {
            Ok(Some(t)) => Ok(t),
            Ok(None) => Ok(Throttle::default()),
            Err(_) => {
                let mut t = Throttle::default();
                t.entries.insert(
                    EVERYONE.to_owned(),
                    Entry {
                        failures: 1,
                        until_ms: now.saturating_add(MAX_WAIT_MS),
                    },
                );
                Ok(t)
            }
        }
    }

    /// Stores the counters atomically.
    pub fn save(&self, dir: &Path) -> Result<(), FileError> {
        file::write(&dir.join(FILE), KIND, &self.encode())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_double_and_stop_at_the_maximum() {
        assert_eq!(wait_after(0), 0);
        assert_eq!(wait_after(1), 1_000);
        assert_eq!(wait_after(2), 2_000);
        assert_eq!(wait_after(4), 8_000);
        assert_eq!(wait_after(9), 256_000);
        assert_eq!(wait_after(10), MAX_WAIT_MS);
        assert_eq!(wait_after(u32::MAX), MAX_WAIT_MS);
    }

    #[test]
    fn failures_count_per_key_and_success_clears() {
        let mut t = Throttle::default();
        assert_eq!(t.fail("anas", 100), 1_000);
        assert_eq!(t.remaining("anas", 600), 500);
        assert_eq!(t.remaining("leila", 600), 0);
        assert_eq!(t.fail("anas", 2_000), 2_000);
        assert_eq!(t.failures("anas"), 2);
        t.succeed("anas");
        assert_eq!(t.remaining("anas", 0), 0);
        let mut far = Throttle::default();
        far.fail("x", 10_000_000);
        assert_eq!(far.remaining("x", 0), MAX_WAIT_MS, "clock went back");
    }

    #[test]
    fn counters_round_trip_and_are_bounded() {
        let mut t = Throttle::default();
        t.fail("anas", 5);
        t.fail("*", 7);
        assert_eq!(Throttle::decode(&t.encode()).unwrap(), t);
        for bad in [
            "fail a 1\n",
            "fail a x 1\n",
            "nope a 1 2\n",
            "fail a 1 2 3\n",
        ] {
            assert!(Throttle::decode(bad).is_err(), "{bad}");
        }
        let mut many = Throttle::default();
        for i in 0..MAX_KEYS + 20 {
            many.fail(&format!("u{i}"), i as u64);
        }
        assert_eq!(many.entries.len(), MAX_KEYS);
    }
}
