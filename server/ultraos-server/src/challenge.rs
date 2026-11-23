//! Single-use challenges for proofs of possession.
//!
//! A challenge is 32 bytes, bound when it is issued to a purpose, an account and the public key
//! that claims it, and lives for [`CHALLENGE_TTL_MS`]. Two kinds exist:
//!
//! * **Account challenges** (register, upgrade, replace, recover, approve – requested with a
//!   token): stored while pending, at most [`MAX_PER_OWNER`] per account. The first presentation
//!   removes the pending entry, whatever the outcome; a small marker of it (at most
//!   [`MAX_USED_PER_OWNER`] per account, the account's oldest dropped first) only lets a second
//!   presentation answer `Used` instead of `Unknown`. A replay is refused either way.
//! * **Session challenges** (requested without a token, by naming a public key): stateless.
//!   Their id carries purpose, account, key, issue time and a random nonce, authenticated with
//!   HMAC-SHA256 under a per-process key ([`crate::challenge_session`]); issuing stores nothing,
//!   so requests of strangers who know a key change nothing for the device. A session challenge
//!   is consumed only by a presentation whose signature verifies (its nonce is remembered until
//!   it expires, at most [`MAX_SESSIONS_PER_KEY`] per key).
//!
//! Everything is in memory; a restart forgets all of it (fail closed: the process key is new).

use crate::identity::{CHALLENGE_BYTES, PUBLIC_KEY_BYTES};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Mutex, MutexGuard};

/// Length of the id of an account challenge in bytes.
pub const CHALLENGE_ID_BYTES: usize = 16;
/// Length of the id of a session challenge in bytes.
pub const SESSION_ID_BYTES: usize = crate::challenge_session::SESSION_ID_BYTES;
/// Longest challenge id in bytes.
pub const MAX_ID_BYTES: usize = SESSION_ID_BYTES;
/// Lifetime of a challenge in milliseconds.
pub const CHALLENGE_TTL_MS: u64 = 120_000;
/// Most pending account challenges of one account.
pub const MAX_PER_OWNER: u32 = 8;
/// Most remembered presentations of account challenges per account.
pub const MAX_USED_PER_OWNER: usize = 8;
/// Most accepted session challenges per key within one lifetime.
pub const MAX_SESSIONS_PER_KEY: usize = 16;
/// Most stored entries in total (pending account challenges and markers of presented ones).
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
    /// Too many challenges are pending or were presented.
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
            ChallengeError::Capacity { .. } => "too many challenges",
        })
    }
}

impl std::error::Error for ChallengeError {}

/// A freshly issued challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issued {
    /// Handle the device sends back (16 bytes for account challenges, longer for sessions).
    pub id: Vec<u8>,
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

/// A session challenge that was opened and not yet consumed: consume it with
/// [`ChallengeStore::confirm`] once the signature verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket {
    /// What the challenge is bound to.
    pub bound: Bound,
    nonce: [u8; crate::challenge_session::NONCE_BYTES],
    issued_ms: u64,
}

pub(crate) struct Pending {
    /// What it was issued for.
    pub(crate) purpose: ChallengeUse,
    pub(crate) bound: Bound,
    pub(crate) issued_ms: u64,
}

/// The store's tables.
#[derive(Default)]
pub(crate) struct Inner {
    pub(crate) pending: HashMap<[u8; CHALLENGE_ID_BYTES], Pending>,
    pub(crate) pending_per_account: HashMap<i64, u32>,
    /// Presented account challenges: id -> issue time; per account in order of presentation.
    pub(crate) used: HashMap<[u8; CHALLENGE_ID_BYTES], u64>,
    pub(crate) used_per_account: HashMap<i64, VecDeque<([u8; CHALLENGE_ID_BYTES], u64)>>,
    /// Accepted session challenges: nonce -> issue time; per key in order of acceptance.
    pub(crate) sessions: HashMap<[u8; crate::challenge_session::NONCE_BYTES], u64>,
    pub(crate) sessions_per_key: HashMap<[u8; PUBLIC_KEY_BYTES], VecDeque<u64>>,
    pub(crate) oldest_ms: u64,
}

impl Inner {
    pub(crate) fn total(&self) -> usize {
        self.pending.len() + self.used.len() + self.sessions.len()
    }
}

/// The bounded in-memory store of challenges.
pub struct ChallengeStore {
    inner: Mutex<Inner>,
    codec: crate::challenge_session::SessionCodec,
    ttl_ms: u64,
    max_per_owner: u32,
    max_total: usize,
}

impl Default for ChallengeStore {
    fn default() -> Self {
        Self::new(CHALLENGE_TTL_MS, MAX_PER_OWNER, MAX_TOTAL)
    }
}

/// Lowers a count by one and drops it at zero.
pub(crate) fn decrement_count<K: std::hash::Hash + Eq>(counts: &mut HashMap<K, u32>, key: &K) {
    if let Some(n) = counts.get_mut(key) {
        *n = n.saturating_sub(1);
        if *n == 0 {
            counts.remove(key);
        }
    }
}

impl ChallengeStore {
    /// A store with explicit lifetime and bounds.
    pub fn new(ttl_ms: u64, max_per_owner: u32, max_total: usize) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            codec: crate::challenge_session::SessionCodec::random(),
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

    fn wait_secs(&self, since_ms: u64, now_ms: u64) -> u64 {
        self.ttl_ms
            .saturating_sub(now_ms.saturating_sub(since_ms))
            .div_ceil(1000)
            .max(1)
    }

    /// Issues a challenge bound to `purpose`, `account_id` and `public_key`. Session challenges
    /// are never refused (nothing is stored for them).
    pub fn issue(
        &self,
        purpose: ChallengeUse,
        account_id: i64,
        public_key: [u8; PUBLIC_KEY_BYTES],
        now_ms: u64,
    ) -> Result<Issued, ChallengeError> {
        if purpose == ChallengeUse::Session {
            return Ok(self.codec.issue(account_id, public_key, now_ms));
        }
        let mut inner = self.lock();
        crate::challenge_purge::purge(&mut inner, self.ttl_ms, now_ms);
        let held = inner
            .pending_per_account
            .get(&account_id)
            .copied()
            .unwrap_or(0);
        if held >= self.max_per_owner {
            let oldest = inner
                .pending
                .values()
                .filter(|p| p.bound.account_id == account_id)
                .map(|p| p.issued_ms)
                .min()
                .unwrap_or(now_ms);
            return Err(ChallengeError::Capacity {
                retry_after_secs: self.wait_secs(oldest, now_ms),
            });
        }
        if inner.total() >= self.max_total
            && !crate::challenge_purge::drop_oldest_marker(&mut inner)
        {
            return Err(ChallengeError::Capacity {
                retry_after_secs: self.wait_secs(inner.oldest_ms, now_ms),
            });
        }
        let issued = crate::challenge_session::random_account_challenge(&inner.pending);
        if inner.total() == 0 {
            inner.oldest_ms = now_ms;
        }
        let bound = Bound {
            account_id,
            public_key,
            challenge: issued.1,
        };
        let entry = Pending {
            purpose,
            bound,
            issued_ms: now_ms,
        };
        inner.pending.insert(issued.0, entry);
        *inner.pending_per_account.entry(account_id).or_insert(0) += 1;
        Ok(Issued {
            id: issued.0.to_vec(),
            challenge: issued.1,
        })
    }

    /// Consumes a challenge. `account_id` is `None` when the caller learns the account from the
    /// challenge itself (sessions). An account challenge is used up by any presentation; a
    /// session challenge presented here is consumed without a signature check – the HTTP layer
    /// uses [`ChallengeStore::open_session`] and [`ChallengeStore::confirm`] instead.
    pub fn take(
        &self,
        id: &[u8],
        purpose: ChallengeUse,
        account_id: Option<i64>,
        now_ms: u64,
    ) -> Result<Bound, ChallengeError> {
        if id.len() == SESSION_ID_BYTES {
            let ticket = self.open_session(id, purpose, account_id, now_ms)?;
            self.confirm(&ticket, now_ms)?;
            return Ok(ticket.bound);
        }
        let id = <[u8; CHALLENGE_ID_BYTES]>::try_from(id).map_err(|_| ChallengeError::Unknown)?;
        let mut inner = self.lock();
        if inner.used.contains_key(&id) {
            return Err(ChallengeError::Used);
        }
        let entry = inner.pending.remove(&id).ok_or(ChallengeError::Unknown)?;
        decrement_count(&mut inner.pending_per_account, &entry.bound.account_id);
        if now_ms.saturating_sub(entry.issued_ms) >= self.ttl_ms {
            return Err(ChallengeError::Expired);
        }
        crate::challenge_purge::remember_used(&mut inner, id, &entry.bound, entry.issued_ms);
        let account_ok = account_id.is_none_or(|a| a == entry.bound.account_id);
        if entry.purpose != purpose || !account_ok {
            return Err(ChallengeError::Mismatch);
        }
        Ok(entry.bound)
    }

    /// Checks a session challenge without consuming it: authentic, not expired, not used, for
    /// this purpose and (if given) account.
    pub fn open_session(
        &self,
        id: &[u8],
        purpose: ChallengeUse,
        account_id: Option<i64>,
        now_ms: u64,
    ) -> Result<Ticket, ChallengeError> {
        let opened = self.codec.open(id).ok_or(ChallengeError::Unknown)?;
        if opened.issued_ms > now_ms {
            return Err(ChallengeError::Unknown);
        }
        if now_ms - opened.issued_ms >= self.ttl_ms {
            return Err(ChallengeError::Expired);
        }
        if self.lock().sessions.contains_key(&opened.nonce) {
            return Err(ChallengeError::Used);
        }
        let account_ok = account_id.is_none_or(|a| a == opened.bound.account_id);
        if purpose != ChallengeUse::Session || !account_ok {
            return Err(ChallengeError::Mismatch);
        }
        Ok(Ticket {
            bound: opened.bound,
            nonce: opened.nonce,
            issued_ms: opened.issued_ms,
        })
    }

    /// Consumes an opened session challenge (after its signature verified). Exactly one of
    /// several concurrent confirmations of the same challenge succeeds.
    pub fn confirm(&self, ticket: &Ticket, now_ms: u64) -> Result<(), ChallengeError> {
        let mut inner = self.lock();
        crate::challenge_purge::purge(&mut inner, self.ttl_ms, now_ms);
        if inner.sessions.contains_key(&ticket.nonce) {
            return Err(ChallengeError::Used);
        }
        let key = ticket.bound.public_key;
        let per_key = inner.sessions_per_key.get(&key);
        if let Some(oldest) = per_key.filter(|q| q.len() >= MAX_SESSIONS_PER_KEY) {
            let since = oldest.front().copied().unwrap_or(now_ms);
            return Err(ChallengeError::Capacity {
                retry_after_secs: self.wait_secs(since, now_ms),
            });
        }
        if inner.total() >= self.max_total
            && !crate::challenge_purge::drop_oldest_marker(&mut inner)
        {
            return Err(ChallengeError::Capacity {
                retry_after_secs: self.wait_secs(inner.oldest_ms, now_ms),
            });
        }
        if inner.total() == 0 {
            inner.oldest_ms = ticket.issued_ms;
        }
        inner.sessions.insert(ticket.nonce, ticket.issued_ms);
        let queue = inner.sessions_per_key.entry(key).or_default();
        queue.push_back(ticket.issued_ms);
        Ok(())
    }

    /// Number of stored entries (pending account challenges, markers of presented ones,
    /// accepted session nonces).
    pub fn len(&self) -> usize {
        self.lock().total()
    }

    /// True when nothing is stored.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
#[path = "challenge_tests.rs"]
mod tests;
