//! Counters of a group as the file views and the log read them.

use super::{chain, slot, DYING, FIXED, MAX_GROUPS};
use core::sync::atomic::{AtomicU64, Ordering};

/// Snapshot of a group's counters for the file views.
pub struct Usage {
    /// Bytes charged now / highest value seen.
    pub mem: (u64, u64),
    /// Threads now / highest value seen.
    pub pids: (u64, u64),
    /// Refusals by `memory.max`, kills for lack of memory, refusals by `pids.max`.
    pub events: [u64; 3],
}

/// Counters of `g`.
pub fn usage(g: u16) -> Usage {
    let s = slot(g);
    let l = |a: &AtomicU64| a.load(Ordering::Relaxed);
    Usage {
        mem: (l(&s.mem), l(&s.mem_peak)),
        pids: (l(&s.pids), l(&s.pids_peak)),
        events: [l(&s.events[0]), l(&s.events[1]), l(&s.events[2])],
    }
}

/// Note that the kernel ended processes of `g` for lack of memory.
pub fn count_kill(g: u16) {
    for x in chain(g) {
        slot(x).events[1].fetch_add(1, Ordering::Relaxed);
    }
}

/// Removed groups that still have memory charged (objects that outlived their processes), with the
/// sum of those bytes. Both are 0 when nothing leaked.
pub fn lingering() -> (u64, u64) {
    let mut out = (0, 0);
    for g in FIXED..MAX_GROUPS as u16 {
        let s = slot(g);
        if s.state.load(Ordering::Relaxed) == DYING {
            out.0 += 1;
            out.1 += s.mem.load(Ordering::Relaxed);
        }
    }
    out
}
