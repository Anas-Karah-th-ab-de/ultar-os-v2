//! What the kernel does when memory runs out – instead of stopping the machine.
//!
//! * A request with policy that is refused inside a system call becomes `ENOMEM` (see `heap.rs`,
//!   `syscall::dispatch`).
//! * A page fault that cannot be satisfied ends the faulting process like `SIGKILL` ([`fault`]) when
//!   its own group is the reason (explicit limit, protected minimum, fair share). When the machine as
//!   a whole is short, the kernel first ends the victim chosen by `groups::policy` and lets the
//!   faulting process try again.
//! * [`watch`] runs at every scheduling decision and after every system call: below the critical
//!   watermark it ends the victim. One victim at a time – the next one is chosen only after the
//!   memory of the previous one has been released or [`HOLD_MS`] have passed.
//! * [`after_syscall`] ends a process whose system call pushed its group above an explicit
//!   `memory.max` with small kernel objects (those are charged without asking).

use crate::groups::policy::{self, Victim};
use crate::groups::{self, Denied};
use crate::proc::{self, Proc, State, PROCS};
use crate::trap::TrapFrame;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

/// Status of a process ended for lack of memory: killed by signal 9.
pub const KILLED: i32 = 9;
/// Time the kernel waits for a victim's memory before it chooses the next one.
pub const HOLD_MS: u64 = 250;

static HOLD_UNTIL: AtomicU64 = AtomicU64::new(0);
/// One processor at a time decides about a victim.
static DECIDING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static KILLS: AtomicU64 = AtomicU64::new(0);

/// How many times the kernel ended processes for lack of memory since start.
pub fn kills() -> u64 {
    KILLS.load(Ordering::Relaxed)
}

fn alive(p: &Proc) -> bool {
    p.pid == p.tgid && !matches!(p.state, State::Zombie(_) | State::Dead) && p.killed.is_none()
}

/// Thread groups to end for `victim`, with the name of what is ended.
fn targets(ps: &[Box<Proc>], victim: Victim) -> (Vec<u64>, u16) {
    match victim {
        Victim::Group(g) => (ps.iter().filter(|p| alive(p) && groups::within(p.g.res, g)).map(|p| p.tgid).collect(), g),
        Victim::LargestOfSession => {
            let size = |p: &Proc| p.g.space.as_ref().map(|s| s.resident.load(Ordering::Relaxed)).unwrap_or(0);
            let best = ps.iter().filter(|p| alive(p) && groups::branch_of(p.g.res) == groups::SESSION).max_by_key(|p| (size(p), p.pid));
            (best.map(|p| p.tgid).into_iter().collect(), groups::SESSION)
        }
    }
}

/// End the victim now. Returns the number of processes ended; 0 if there is nobody the kernel may
/// end (only `system` is left) or the previous victim is still being released.
fn kill_victim() -> usize {
    // Two processors that both find memory critical must not both choose: the second would pick the
    // *next* victim before the first one's memory is back. The loser waits like during the hold.
    if DECIDING.swap(true, Ordering::Acquire) {
        return usize::MAX;
    }
    let n = choose_and_kill();
    DECIDING.store(false, Ordering::Release);
    n
}

fn choose_and_kill() -> usize {
    let now = crate::arch::uptime_ms();
    if now < HOLD_UNTIL.load(Ordering::Relaxed) {
        return usize::MAX;
    }
    let free = crate::mem::free_bytes();
    let (n, g, used, whole) = {
        let mut ps = PROCS.lock();
        let victim = policy::victim(&|g| ps.iter().any(|p| alive(p) && groups::within(p.g.res, g)));
        let (tgids, g) = targets(&ps, victim);
        // a group is ended as a whole; of the session only its largest process (pages × 4 KiB)
        let whole = matches!(victim, Victim::Group(_));
        let resident = |t: &u64| ps.iter().find(|p| p.pid == *t).and_then(|p| p.g.space.as_ref()).map(|s| s.resident.load(Ordering::Relaxed) * 4096);
        let used = if whole { groups::memory(g) } else { tgids.first().and_then(resident).unwrap_or(0) };
        for t in &tgids {
            proc::kill_group(&mut ps, *t, KILLED);
        }
        if !tgids.is_empty() {
            HOLD_UNTIL.store(now + HOLD_MS, Ordering::Relaxed);
        }
        (tgids.len(), g, used, whole)
    };
    if n == 0 {
        return 0;
    }
    KILLS.fetch_add(1, Ordering::Relaxed);
    groups::count_kill(g);
    crate::heap::kernel(|| {
        // log codes, not sentences: `oom-kill group` = the whole group, `oom-kill largest-of` = one process
        let what = if whole { "group" } else { "largest-of" };
        crate::kprintln!("[mem] oom-kill {} {} free={}KiB procs={} used={}KiB", what, groups::files::path_of(g), free >> 10, n, used >> 10);
    });
    n
}

/// Called where no lock is held: end the victim if free memory is below the critical watermark.
pub fn watch() {
    if policy::must_kill() {
        kill_victim();
    }
}

fn me_name() -> (String, u16) {
    let me = proc::current();
    PROCS.lock().iter().find(|p| p.pid == me).map(|p| (p.name.clone(), p.g.res)).unwrap_or_default()
}

/// A page fault of the running process could not get a page. Never returns to the faulting
/// instruction directly: the process ends, or it is rescheduled and faults again later.
pub fn fault(f: &TrapFrame) -> ! {
    let reason = crate::heap::last_denial().unwrap_or(Denied::Reserve);
    if reason == Denied::Reserve {
        // the machine is short, not this group: end the worst offender and try again
        let n = kill_victim();
        let me = proc::current();
        let dying = PROCS.lock().iter().find(|p| p.pid == me).and_then(|p| p.killed);
        if let Some(st) = dying {
            proc::exit_thread(st, true);
        }
        if n != 0 {
            proc::switch_out(f, None);
        }
    }
    let why = match reason {
        Denied::Limit => "group-limit",
        Denied::Share => "over-share",
        Denied::Reserve => "no-memory",
    };
    end_me(why)
}

/// Log why (a code: `group-limit`, `over-share`, `no-memory`, `over-limit`), count, and end the
/// running process. (The text is built and freed in an inner scope:
/// nothing on this stack is ever dropped once the process is gone.)
fn end_me(why: &str) -> ! {
    crate::heap::kernel(|| {
        let (name, g) = me_name();
        crate::kprintln!("[mem] oom-end {} pid={} group={} reason={}", name, proc::current(), groups::files::path_of(g), why);
        groups::count_kill(g);
    });
    KILLS.fetch_add(1, Ordering::Relaxed);
    proc::exit_current(KILLED)
}

/// After a system call of a process in group `g` that had `before` bytes charged when the call
/// began (`u64::MAX`: no group has a limit): end the process if the call left the group above an
/// explicit limit; then [`watch`].
pub fn after_syscall(g: u16, before: u64) {
    if before != u64::MAX && groups::over_limit(g) && groups::memory(g) > before {
        end_me("over-limit");
    }
    watch();
}
