//! Dynamic protection: the rules that hold where no explicit limit is configured.
//!
//! All numbers are derived from the machine's memory `T` at start (see [`init`]); nothing here
//! bites while memory is plentiful.
//!
//! | Name | Value | Meaning |
//! |---|---|---|
//! | reserve | `T/64`, 4 MiB … 64 MiB (`mem::reserve_bytes`) | only the kernel's own small objects go below |
//! | critical | reserve × 1.5 | below: the kernel ends the worst offender |
//! | low | reserve + `T/16` (16 … 256 MiB) | below: memory is "short", the fair-share rule applies |
//! | protected(system) | `T/16`, 16 … 512 MiB | the other branches cannot consume this minimum |
//! | protected(session) | `T/8`, 32 MiB … 1 GiB | `apps` and `cells` cannot consume this minimum |
//! | threads | `T/64 KiB`, 256 … 65536 | ceiling of the whole machine; ½ per branch, ⅛ per group |
//!
//! A request with policy (user pages, page tables, every fallible kernel buffer) of `n` bytes by a
//! group in branch `B` is refused when one of these holds, checked in this order:
//!
//! 1. `free − n < reserve` → [`Denied::Reserve`].
//! 2. `B` is not `system`: `free − n < reserve + unused protected minimum` of `system` (and of
//!    `session` if `B` is `apps` or `cells`) → [`Denied::Share`]. "Unused" = protected − charged.
//! 3. `B` is not `system`, `free − n < low`, and the requester's *unit* is the [`offender`]
//!    → [`Denied::Share`].
//!
//! Units are `session` as a whole and every child group of `apps` and `cells`. With `M` units that
//! hold memory and `S = T − reserve − protected(system) − protected(session)`, a child's fair share
//! is `S/M` and the session's is `protected(session) + S/M`. The offender is the unit with the
//! largest `charged / share`; equal ratios: the higher slot number (the younger group).
//!
//! When free memory falls below *critical* the kernel kills ([`victim`]): the offender among the
//! children of `cells` and `apps` that have processes – all its processes; only if there is none,
//! the largest single process of `session`. Processes of `system` are never chosen.

use super::{branch_of, for_children, memory, parent, pids, top_of, Denied, APPS, CELLS, ROOT, SESSION, SYSTEM};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static TOTAL: AtomicU64 = AtomicU64::new(0);
static LOW: AtomicU64 = AtomicU64::new(0);
static CRITICAL: AtomicU64 = AtomicU64::new(0);
static PROTECT_SYSTEM: AtomicU64 = AtomicU64::new(0);
static PROTECT_SESSION: AtomicU64 = AtomicU64::new(0);
static THREADS: AtomicU64 = AtomicU64::new(256);
static EMERGENCY: AtomicBool = AtomicBool::new(false);
/// With at least this much free memory no rule can refuse anything (reserve + both protected
/// minimums, or the low watermark if that is higher): the common case is one comparison.
static PLENTY: AtomicU64 = AtomicU64::new(u64::MAX);

const MIB: u64 = 1 << 20;

/// Derive the rules from `total` bytes of memory.
pub fn init(total: u64) {
    let reserve = crate::mem::reserve_bytes();
    TOTAL.store(total, Ordering::Relaxed);
    CRITICAL.store(reserve + reserve / 2, Ordering::Relaxed);
    LOW.store(reserve + (total / 16).clamp(16 * MIB, 256 * MIB).min(total / 4), Ordering::Relaxed);
    PROTECT_SYSTEM.store((total / 16).clamp(16 * MIB, 512 * MIB).min(total / 8), Ordering::Relaxed);
    PROTECT_SESSION.store((total / 8).clamp(32 * MIB, 1024 * MIB).min(total / 4), Ordering::Relaxed);
    THREADS.store((total / (64 << 10)).clamp(256, 65536), Ordering::Relaxed);
    PLENTY.store((reserve + protected(SYSTEM) + protected(SESSION)).max(low()), Ordering::Relaxed);
}

/// Low watermark in bytes.
pub fn low() -> u64 {
    LOW.load(Ordering::Relaxed)
}

/// Critical watermark in bytes.
pub fn critical() -> u64 {
    CRITICAL.load(Ordering::Relaxed)
}

/// Protected minimum of a branch in bytes (0 for `apps`, `cells` and their groups).
pub fn protected(g: u16) -> u64 {
    match g {
        SYSTEM => PROTECT_SYSTEM.load(Ordering::Relaxed),
        SESSION => PROTECT_SESSION.load(Ordering::Relaxed),
        _ => 0,
    }
}

/// Dynamic ceiling of threads for a group: the whole machine for the root and `system`, half of it
/// for each other branch, an eighth for every group below a branch.
pub fn dynamic_pids(g: u16) -> u64 {
    let all = THREADS.load(Ordering::Relaxed);
    if g == ROOT || g == SYSTEM {
        all
    } else if parent(g) == ROOT {
        all / 2
    } else {
        all / 8
    }
}

fn unused(branch: u16) -> u64 {
    protected(branch).saturating_sub(memory(branch))
}

/// Decide a request of `n` bytes by group `g` (rules 1–3 of the module description).
pub fn check(g: u16, n: u64) -> Result<(), Denied> {
    let free = crate::mem::free_bytes();
    if free >= n.saturating_add(PLENTY.load(Ordering::Relaxed)) {
        return Ok(());
    }
    let reserve = crate::mem::reserve_bytes();
    if free < n + reserve {
        return Err(Denied::Reserve);
    }
    let branch = branch_of(g);
    if branch == ROOT || branch == SYSTEM {
        return Ok(());
    }
    let after = free - n;
    let kept = unused(SYSTEM) + if branch == SESSION { 0 } else { unused(SESSION) };
    if after < reserve + kept {
        return Err(Denied::Share);
    }
    let unit = if branch == SESSION { SESSION } else { top_of(g) };
    if after < low() && offender(None) == Some(unit) {
        return Err(Denied::Share);
    }
    Ok(())
}

/// The unit furthest above its fair share (see the module description). With `has_processes`
/// (choice of a victim): leave the session out and consider only groups for which it answers yes.
pub fn offender(has_processes: Option<&dyn Fn(u16) -> bool>) -> Option<u16> {
    let mut units = 0u64;
    let count = |g: u16| (memory(g) > 0 || pids(g) > 0) as u64;
    for_children(APPS, |g| units += count(g));
    for_children(CELLS, |g| units += count(g));
    let total = TOTAL.load(Ordering::Relaxed);
    let pool = total.saturating_sub(crate::mem::reserve_bytes() + protected(SYSTEM) + protected(SESSION));
    let share = (pool / (units + 1)).max(1);
    let mut best: Option<(u128, u16)> = None;
    let mut consider = |g: u16, share: u64| {
        let ratio = (memory(g) as u128) << 20;
        let ratio = ratio / share as u128;
        if best.map_or(true, |(r, b)| ratio > r || ratio == r && g > b) {
            best = Some((ratio, g));
        }
    };
    if has_processes.is_none() {
        consider(SESSION, protected(SESSION) + share);
    }
    for branch in [APPS, CELLS] {
        for_children(branch, |g| {
            if has_processes.map_or(true, |f| f(g)) {
                consider(g, share);
            }
        });
    }
    best.map(|b| b.1)
}

/// The allocators had to use the emergency heap: treat memory as critical until the next kill.
pub fn set_critical() {
    EMERGENCY.store(true, Ordering::Relaxed);
}

/// Whether the kernel must end a process now to get memory back.
pub fn must_kill() -> bool {
    EMERGENCY.load(Ordering::Relaxed) || crate::mem::free_bytes() < critical()
}

/// Who is ended when memory is critical.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Victim {
    /// Every process of this group (a child of `cells` or `apps`) and of the groups below it.
    Group(u16),
    /// The largest single process of the `session` branch.
    LargestOfSession,
}

/// Choose the victim; clears the emergency mark (the caller acts on the answer).
/// `has_processes(g)`: whether group `g` or a group below it has a process that can be ended.
pub fn victim(has_processes: &dyn Fn(u16) -> bool) -> Victim {
    EMERGENCY.store(false, Ordering::Relaxed);
    match offender(Some(has_processes)) {
        Some(g) => Victim::Group(g),
        None => Victim::LargestOfSession,
    }
}
