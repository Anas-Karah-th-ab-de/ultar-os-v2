//! Housekeeping of the challenge store: expiry, the bounded markers of presented account
//! challenges, and making room when the store is full. Part of [`crate::challenge`].

use crate::challenge::{Bound, Inner, CHALLENGE_ID_BYTES, MAX_USED_PER_OWNER};

/// Drops everything older than its lifetime. Cheap when nothing can have expired yet.
pub(crate) fn purge(inner: &mut Inner, ttl_ms: u64, now_ms: u64) {
    if inner.total() == 0 || now_ms.saturating_sub(inner.oldest_ms) < ttl_ms {
        return;
    }
    let live = |issued: u64| now_ms.saturating_sub(issued) < ttl_ms;
    let Inner {
        pending,
        pending_per_account,
        used,
        used_per_account,
        sessions,
        sessions_per_key,
        oldest_ms,
    } = inner;
    pending.retain(|_, p| {
        let keep = live(p.issued_ms);
        if !keep {
            crate::challenge::decrement_count(pending_per_account, &p.bound.account_id);
        }
        keep
    });
    used.retain(|_, issued| live(*issued));
    used_per_account.retain(|_, q| {
        q.retain(|(_, issued)| live(*issued));
        !q.is_empty()
    });
    sessions.retain(|_, issued| live(*issued));
    sessions_per_key.retain(|_, q| {
        q.retain(|issued| live(*issued));
        !q.is_empty()
    });
    let oldest = pending
        .values()
        .map(|p| p.issued_ms)
        .chain(used.values().copied())
        .chain(sessions.values().copied())
        .min();
    *oldest_ms = oldest.unwrap_or(now_ms);
}

/// Remembers a presented account challenge; the account's oldest marker goes when it has
/// [`MAX_USED_PER_OWNER`] already. Markers only choose the error code of a replay.
pub(crate) fn remember_used(
    inner: &mut Inner,
    id: [u8; CHALLENGE_ID_BYTES],
    bound: &Bound,
    issued_ms: u64,
) {
    let queue = inner.used_per_account.entry(bound.account_id).or_default();
    queue.push_back((id, issued_ms));
    let mut dropped = Vec::new();
    while queue.len() > MAX_USED_PER_OWNER {
        if let Some((old, _)) = queue.pop_front() {
            dropped.push(old);
        }
    }
    for old in dropped {
        inner.used.remove(&old);
    }
    inner.used.insert(id, issued_ms);
}

/// Makes room for one entry by dropping the oldest marker of a presented account challenge.
/// False when there is none (the store is full of pending challenges and accepted sessions).
pub(crate) fn drop_oldest_marker(inner: &mut Inner) -> bool {
    let oldest = inner
        .used_per_account
        .iter()
        .filter_map(|(account, q)| q.front().map(|(id, issued)| (*issued, *account, *id)))
        .min();
    let Some((_, account, id)) = oldest else {
        return false;
    };
    inner.used.remove(&id);
    if let Some(q) = inner.used_per_account.get_mut(&account) {
        q.pop_front();
        if q.is_empty() {
            inner.used_per_account.remove(&account);
        }
    }
    true
}
