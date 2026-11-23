//! Single-use challenges for proofs of possession.
//!
//! A challenge is 32 random bytes, bound when it is issued to a purpose, an account and the public
//! key that claims it. It is addressed by a separate random id, lives for a short time, and the
//! first presentation consumes it whatever the outcome. Pending challenges exist in memory only;
//! their number is bounded per owner and in total, and nothing is evicted before it expires.

use crate::identity::{CHALLENGE_BYTES, PUBLIC_KEY_BYTES};
use rand::RngCore;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, MutexGuard};

/// Length of a challenge id in bytes.
pub const CHALLENGE_ID_BYTES: usize = 16;
/// Lifetime of a challenge in milliseconds.
pub const CHALLENGE_TTL_MS: u64 = 120_000;
/// Most pending challenges of one account (or, for sessions, of one key).
pub const MAX_PER_OWNER: u32 = 8;
/// Most pending challenges in total.
pub const MAX_TOTAL: usize = 65_536;

/// What a challenge was issued for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChallengeUse {
    /// Registration of a new key.
    Register,
    /// Issue of a device token.
    Session,
    /// Upgrade of a registration from before device keys.
    Upgrade,
    /// Replacement signed by the old and the new device.
    Replace,
    /// Replacement of a lost device.
    Recover,
}

impl ChallengeUse {
    /// Parses the `purpose` field of a challenge request.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "register" => Some(ChallengeUse::Register),
            "session" => Some(ChallengeUse::Session),
            "upgrade" => Some(ChallengeUse::Upgrade),
            "replace" => Some(ChallengeUse::Replace),
            "recover" => Some(ChallengeUse::Recover),
            _ => None,
        }
    }
}

/// Why a challenge was not issued or not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeError {
    /// No challenge with this id (never issued, or forgotten).
    Unknown,
    /// The challenge was presented before.
    Used,
    /// The challenge is older than its lifetime.
    Expired,
    /// The challenge was issued for another purpose or account.
    Mismatch,
    /// Too many challenges are pending.
    Capacity {
        /// Seconds until room is certain to be free again.
        retry_after_secs: u64,
    },
}

impl fmt::Display for ChallengeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ChallengeError::Unknown => "unknown challenge",
            ChallengeError::Used => "challenge already used",
            ChallengeError::Expired => "challenge expired",
            ChallengeError::Mismatch => "challenge issued for another purpose or account",
            ChallengeError::Capacity { .. } => "too many pending challenges",
        })
    }
}

impl std::error::Error for ChallengeError {}

/// A freshly issued challenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Issued {
    /// Handle the device sends back.
    pub id: [u8; CHALLENGE_ID_BYTES],
    /// The bytes that enter the transcript.
    pub challenge: [u8; CHALLENGE_BYTES],
}

/// What an accepted challenge was bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bound {
    /// Account it was issued for.
    pub account_id: i64,
    /// Public key that claimed it.
    pub public_key: [u8; PUBLIC_KEY_BYTES],
    /// The challenge bytes.
    pub challenge: [u8; CHALLENGE_BYTES],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Owner {
    Account(i64),
    Key([u8; PUBLIC_KEY_BYTES]),
}

struct Entry {
    purpose: ChallengeUse,
    owner: Owner,
    bound: Bound,
    issued_ms: u64,
    used: bool,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<[u8; CHALLENGE_ID_BYTES], Entry>,
    per_owner: HashMap<Owner, u32>,
    oldest_ms: u64,
}

/// The bounded in-memory store of pending challenges.
pub struct ChallengeStore {
    inner: Mutex<Inner>,
    ttl_ms: u64,
    max_per_owner: u32,
    max_total: usize,
}

impl Default for ChallengeStore {
    fn default() -> Self {
        Self::new(CHALLENGE_TTL_MS, MAX_PER_OWNER, MAX_TOTAL)
    }
}

impl ChallengeStore {
    /// A store with explicit lifetime and bounds.
    pub fn new(ttl_ms: u64, max_per_owner: u32, max_total: usize) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            ttl_ms,
            max_per_owner,
            max_total,
        }
    }

    /// Lifetime of a challenge in whole seconds.
    pub fn ttl_secs(&self) -> u64 {
        self.ttl_ms / 1000
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn purge(&self, inner: &mut Inner, now_ms: u64) {
        if inner.entries.is_empty() || now_ms.saturating_sub(inner.oldest_ms) < self.ttl_ms {
            return;
        }
        let ttl = self.ttl_ms;
        let Inner {
            entries,
            per_owner,
            oldest_ms,
        } = inner;
        let mut oldest = u64::MAX;
        entries.retain(|_, e| {
            let keep = now_ms.saturating_sub(e.issued_ms) < ttl;
            if keep {
                oldest = oldest.min(e.issued_ms);
            } else if let Some(n) = per_owner.get_mut(&e.owner) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    per_owner.remove(&e.owner);
                }
            }
            keep
        });
        *oldest_ms = if oldest == u64::MAX { now_ms } else { oldest };
    }

    /// Issues a challenge bound to `purpose`, `account_id` and `public_key`.
    pub fn issue(
        &self,
        purpose: ChallengeUse,
        account_id: i64,
        public_key: [u8; PUBLIC_KEY_BYTES],
        now_ms: u64,
    ) -> Result<Issued, ChallengeError> {
        let owner = match purpose {
            ChallengeUse::Session => Owner::Key(public_key),
            _ => Owner::Account(account_id),
        };
        let mut inner = self.lock();
        self.purge(&mut inner, now_ms);
        let held = inner.per_owner.get(&owner).copied().unwrap_or(0);
        if held >= self.max_per_owner || inner.entries.len() >= self.max_total {
            let wait_ms = self
                .ttl_ms
                .saturating_sub(now_ms.saturating_sub(inner.oldest_ms));
            return Err(ChallengeError::Capacity {
                retry_after_secs: wait_ms.div_ceil(1000).max(1),
            });
        }
        let mut issued = Issued {
            id: [0; CHALLENGE_ID_BYTES],
            challenge: [0; CHALLENGE_BYTES],
        };
        let mut rng = rand::rngs::OsRng;
        rng.fill_bytes(&mut issued.challenge);
        loop {
            rng.fill_bytes(&mut issued.id);
            if !inner.entries.contains_key(&issued.id) {
                break;
            }
        }
        if inner.entries.is_empty() {
            inner.oldest_ms = now_ms;
        }
        let bound = Bound {
            account_id,
            public_key,
            challenge: issued.challenge,
        };
        let entry = Entry {
            purpose,
            owner,
            bound,
            issued_ms: now_ms,
            used: false,
        };
        inner.entries.insert(issued.id, entry);
        *inner.per_owner.entry(owner).or_insert(0) += 1;
        Ok(issued)
    }

    /// Consumes a challenge. `account_id` is `None` when the caller learns the account from the
    /// challenge itself (sessions). Any presentation of a pending challenge uses it up.
    pub fn take(
        &self,
        id: &[u8; CHALLENGE_ID_BYTES],
        purpose: ChallengeUse,
        account_id: Option<i64>,
        now_ms: u64,
    ) -> Result<Bound, ChallengeError> {
        let mut inner = self.lock();
        let ttl = self.ttl_ms;
        let entry = inner.entries.get_mut(id).ok_or(ChallengeError::Unknown)?;
        if entry.used {
            return Err(ChallengeError::Used);
        }
        entry.used = true;
        if now_ms.saturating_sub(entry.issued_ms) >= ttl {
            return Err(ChallengeError::Expired);
        }
        let account_ok = account_id.is_none_or(|a| a == entry.bound.account_id);
        if entry.purpose != purpose || !account_ok {
            return Err(ChallengeError::Mismatch);
        }
        Ok(entry.bound)
    }

    /// Number of stored challenges (pending and used ones that have not expired yet).
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    /// True when nothing is stored.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PK: [u8; 32] = [7; 32];

    #[test]
    fn a_challenge_is_accepted_once_for_its_purpose_and_account() {
        let store = ChallengeStore::default();
        let c = store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
        let bound = store
            .take(&c.id, ChallengeUse::Register, Some(1), 10)
            .unwrap();
        assert_eq!((bound.account_id, bound.public_key), (1, PK));
        assert_eq!(bound.challenge, c.challenge);
        assert_eq!(
            store.take(&c.id, ChallengeUse::Register, Some(1), 20),
            Err(ChallengeError::Used)
        );
        assert_eq!(
            store.take(&[0; 16], ChallengeUse::Register, Some(1), 20),
            Err(ChallengeError::Unknown)
        );
    }

    #[test]
    fn challenges_are_random_and_distinct() {
        let store = ChallengeStore::default();
        let a = store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
        let b = store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
        assert_ne!(a.challenge, b.challenge);
        assert_ne!(a.id, b.id);
        assert_ne!(a.challenge, [0; 32]);
    }

    #[test]
    fn wrong_account_or_purpose_is_a_mismatch_and_consumes_the_challenge() {
        let store = ChallengeStore::default();
        let c = store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
        assert_eq!(
            store.take(&c.id, ChallengeUse::Register, Some(2), 1),
            Err(ChallengeError::Mismatch)
        );
        assert_eq!(
            store.take(&c.id, ChallengeUse::Register, Some(1), 2),
            Err(ChallengeError::Used),
            "the failed attempt used it up"
        );
        let c = store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
        assert_eq!(
            store.take(&c.id, ChallengeUse::Replace, Some(1), 1),
            Err(ChallengeError::Mismatch)
        );
        let c = store.issue(ChallengeUse::Session, 5, PK, 0).unwrap();
        assert_eq!(
            store
                .take(&c.id, ChallengeUse::Session, None, 1)
                .unwrap()
                .account_id,
            5
        );
    }

    #[test]
    fn expiry_is_exact_and_expired_entries_are_forgotten() {
        let store = ChallengeStore::default();
        let a = store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
        let b = store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
        assert!(store
            .take(&a.id, ChallengeUse::Register, Some(1), CHALLENGE_TTL_MS - 1)
            .is_ok());
        assert_eq!(
            store.take(&b.id, ChallengeUse::Register, Some(1), CHALLENGE_TTL_MS),
            Err(ChallengeError::Expired)
        );
        assert_eq!(store.len(), 2);
        store
            .issue(ChallengeUse::Register, 2, PK, CHALLENGE_TTL_MS)
            .unwrap();
        assert_eq!(store.len(), 1, "issuing purges what has expired");
        assert_eq!(
            store.take(&a.id, ChallengeUse::Register, Some(1), CHALLENGE_TTL_MS),
            Err(ChallengeError::Unknown)
        );
    }

    #[test]
    fn per_owner_and_total_bounds_hold_and_free_up_after_expiry() {
        let store = ChallengeStore::new(1000, 2, 3);
        store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
        store.issue(ChallengeUse::Upgrade, 1, PK, 100).unwrap();
        assert_eq!(
            store.issue(ChallengeUse::Register, 1, PK, 200),
            Err(ChallengeError::Capacity {
                retry_after_secs: 1
            })
        );
        // Sessions are counted per key, not per account.
        store.issue(ChallengeUse::Session, 1, PK, 300).unwrap();
        assert!(matches!(
            store.issue(ChallengeUse::Register, 2, PK, 400),
            Err(ChallengeError::Capacity { .. })
        ));
        assert_eq!(store.len(), 3);
        // After the lifetime of the first two, their owner has room again.
        store.issue(ChallengeUse::Register, 1, PK, 1100).unwrap();
        store.issue(ChallengeUse::Register, 2, PK, 1100).unwrap();
        assert_eq!(store.len(), 3);
        assert!(!store.is_empty());
        assert_eq!(store.ttl_secs(), 1);
    }

    #[test]
    fn used_challenges_still_count_until_they_expire() {
        let store = ChallengeStore::new(1000, 1, 10);
        let c = store.issue(ChallengeUse::Register, 1, PK, 0).unwrap();
        store
            .take(&c.id, ChallengeUse::Register, Some(1), 1)
            .unwrap();
        assert!(store.issue(ChallengeUse::Register, 1, PK, 2).is_err());
        assert!(store.issue(ChallengeUse::Register, 1, PK, 1000).is_ok());
    }

    #[test]
    fn purposes_parse() {
        for (text, expected) in [
            ("register", ChallengeUse::Register),
            ("session", ChallengeUse::Session),
            ("upgrade", ChallengeUse::Upgrade),
            ("replace", ChallengeUse::Replace),
            ("recover", ChallengeUse::Recover),
        ] {
            assert_eq!(ChallengeUse::parse(text), Some(expected));
        }
        assert_eq!(ChallengeUse::parse("signal"), None);
        assert_eq!(ChallengeUse::parse(""), None);
        assert!(ChallengeError::Used.to_string().contains("used"));
    }
}
