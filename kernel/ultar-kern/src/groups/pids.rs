//! Threads per group: counted when a thread is created, released when it is reaped.

use super::{
    chain, policy, raise_peak, release_if_done, remove, slot, GroupError, MAX_DEPTH, NO_LIMIT, ROOT,
};
use core::sync::atomic::Ordering;

/// One thread counted in a group; dropping it releases the count.
pub struct PidCharge(u16);

impl PidCharge {
    /// Count one more thread in `g` and its ancestors; `None` if a `pids.max` (explicit or dynamic)
    /// is reached.
    pub fn new(g: u16) -> Option<PidCharge> {
        let mut done = 0;
        let mut ok = true;
        for (depth, x) in chain(g).enumerate() {
            let s = slot(x);
            let max = pids_limit(x);
            if s.pids
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |cur| {
                    (cur < max).then_some(cur + 1)
                })
                .is_err()
            {
                s.events[2].fetch_add(1, Ordering::Relaxed);
                ok = false;
                break;
            }
            done = depth + 1;
        }
        if !ok {
            for x in chain(g).take(done) {
                slot(x).pids.fetch_sub(1, Ordering::Relaxed);
            }
            return None;
        }
        for x in chain(g) {
            let s = slot(x);
            raise_peak(&s.pids_peak, s.pids.load(Ordering::Relaxed));
        }
        slot(g).own.fetch_add(1, Ordering::Relaxed);
        Some(PidCharge(g))
    }

    /// Count the thread in `to` instead; fails (unchanged) if `to` has no room.
    pub fn move_to(&mut self, to: u16) -> Result<(), GroupError> {
        if to == self.0 {
            return Ok(());
        }
        let new = PidCharge::new(to).ok_or(GroupError::Again)?;
        drop(core::mem::replace(self, new));
        Ok(())
    }
}

impl Drop for PidCharge {
    fn drop(&mut self) {
        let g = self.0;
        let _ = slot(g)
            .own
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                Some(v.saturating_sub(1))
            });
        let mut cur = g;
        for _ in 0..=MAX_DEPTH + 1 {
            let s = slot(cur);
            let up = s.parent.load(Ordering::Relaxed);
            let _ = s
                .pids
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                    Some(v.saturating_sub(1))
                });
            // a cell's group goes with its last thread (and then its parent cell's, if that is empty too)
            if s.auto.load(Ordering::Relaxed) && s.pids.load(Ordering::Relaxed) == 0 {
                let _ = remove(cur);
            }
            release_if_done(cur);
            if cur == ROOT {
                break;
            }
            cur = up;
        }
    }
}

/// Threads counted in the group (with descendants).
pub fn pids(g: u16) -> u64 {
    slot(g).pids.load(Ordering::Relaxed)
}

/// Effective ceiling of threads for the group: the explicit `pids.max`, else the dynamic one.
pub fn pids_limit(g: u16) -> u64 {
    match slot(g).pids_max.load(Ordering::Relaxed) {
        NO_LIMIT => policy::dynamic_pids(g),
        explicit => explicit,
    }
}
