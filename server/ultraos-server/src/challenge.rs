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
/// Most stored challenges in total (pending ones, and used ones until they expire).
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
    /// An existing device approves a newly added one.
    Approve,
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
            "approve" => Some(ChallengeUse::Approve),
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

    fn release(per_owner: &mut HashMap<Owner, u32>, owner: &Owner) {
        if let Some(n) = per_owner.get_mut(owner) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                per_owner.remove(owner);
            }
        }
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
            } else if !e.used {
                Self::release(per_owner, &e.owner);
            }
            keep
        });
        *oldest_ms = if oldest == u64::MAX { now_ms } else { oldest };
    }

    /// The oldest pending (unused) challenge of `owner`: its id and issue time.
    fn oldest_pending(inner: &Inner, owner: &Owner) -> Option<([u8; CHALLENGE_ID_BYTES], u64)> {
        inner
            .entries
            .iter()
            .filter(|(_, e)| !e.used && e.owner == *owner)
            .min_by_key(|(_, e)| e.issued_ms)
            .map(|(id, e)| (*id, e.issued_ms))
    }

    /// Makes room for one more pending challenge of `owner`, or says how long to wait.
    ///
    /// Challenges of an account are refused beyond the bound (the account asked for them itself).
    /// Session challenges are requested without authentication by naming a public key: there the
    /// oldest pending one is dropped instead, so that requests of a stranger who merely knows the
    /// key cannot lock the device out of its sessions.
    fn make_room(
        &self,
        inner: &mut Inner,
        owner: &Owner,
        now_ms: u64,
    ) -> Result<(), ChallengeError> {
        let capacity = |since_ms: u64| ChallengeError::Capacity {
            retry_after_secs: self
                .ttl_ms
                .saturating_sub(now_ms.saturating_sub(since_ms))
                .div_ceil(1000)
                .max(1),
        };
        let held = inner.per_owner.get(owner).copied().unwrap_or(0);
        if held >= self.max_per_owner {
            let Some((id, issued_ms)) = Self::oldest_pending(inner, owner) else {
                return Err(capacity(now_ms));
            };
            if !matches!(owner, Owner::Key(_)) {
                return Err(capacity(issued_ms));
            }
            inner.entries.remove(&id);
            Self::release(&mut inner.per_owner, owner);
        }
        if inner.entries.len() >= self.max_total {
            return Err(capacity(inner.oldest_ms));
        }
        Ok(())
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
        self.make_room(&mut inner, &owner, now_ms)?;
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
    /// challenge itself (sessions). Any presentation of a pending challenge uses it up and frees
    /// its place in the owner's bound; the entry stays until it expires only to tell a replay
    /// (`Used`) from an unknown id.
    pub fn take(
        &self,
        id: &[u8; CHALLENGE_ID_BYTES],
        purpose: ChallengeUse,
        account_id: Option<i64>,
        now_ms: u64,
    ) -> Result<Bound, ChallengeError> {
        let mut inner = self.lock();
        let ttl = self.ttl_ms;
        let Inner {
            entries, per_owner, ..
        } = &mut *inner;
        let entry = entries.get_mut(id).ok_or(ChallengeError::Unknown)?;
        if entry.used {
            return Err(ChallengeError::Used);
        }
        entry.used = true;
        Self::release(per_owner, &entry.owner);
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
#[path = "challenge_tests.rs"]
mod tests;
