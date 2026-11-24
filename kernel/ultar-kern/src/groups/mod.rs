//! Resource groups of the Ultar kernel (plan §3.7, stage 2): how much a process may *consume*.
//!
//! One tree with four fixed branches below the root – `system` (core services), `session` (the
//! user's surface and programs), `apps` (one group per app) and `cells` (containers, foreign
//! systems). Every further group is a child of a branch (or of another group). A process belongs
//! to exactly one group; memory and threads are counted in that group **and every ancestor**.
//!
//! * Memory: every page and every kernel object is charged by the allocators (`mem.rs`, `heap.rs`)
//!   to the group of the process the kernel works for – anonymous pages, page tables, file and
//!   tmpfs data, pipe buffers, process and descriptor tables alike.
//! * Threads: charged when a thread is created, released when it is reaped ([`PidCharge`]).
//!
//! Limits: an explicit `memory.max` / `pids.max` per group ([`files`] offers them as a
//! `cgroup2`-style tree). Where nothing is configured the *dynamic* rules of [`policy`] protect the
//! machine: they are derived from the amount of memory and only bite when memory is short.
//!
//! The counters are plain atomics in a fixed table, so charging needs no lock and no memory – it
//! is called from inside the allocators.

pub mod files;
mod pids;
pub mod policy;
mod stats;
mod tree;

pub use pids::{pids, pids_limit, PidCharge};
pub use stats::{count_kill, lingering, usage};
pub use tree::{child, create, create_cell, for_children, name, remove};

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, AtomicU8, Ordering};

/// Number of group slots (fixed table).
pub const MAX_GROUPS: usize = 1024;
/// Deepest nesting below the root.
pub const MAX_DEPTH: usize = 6;
/// Longest group name in bytes.
pub const MAX_NAME: usize = 40;
/// "No explicit limit".
pub const NO_LIMIT: u64 = u64::MAX;

/// The root: the kernel itself and the sum of everything.
pub const ROOT: u16 = 0;
/// Core services.
pub const SYSTEM: u16 = 1;
/// The user's surface and the programs started from it.
pub const SESSION: u16 = 2;
/// One child group per app.
pub const APPS: u16 = 3;
/// One child group per cell (container, foreign system).
pub const CELLS: u16 = 4;
const FIXED: u16 = 5;
const FIXED_NAMES: [&str; FIXED as usize] = ["", "system", "session", "apps", "cells"];

const FREE: u8 = 0;
const LIVE: u8 = 1;
const DYING: u8 = 2;

/// Why a request for memory was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Denied {
    /// An explicit `memory.max` of the group or an ancestor.
    Limit = 0,
    /// Dynamic protection: the protected minimum of `system`/`session`, or the group is the one
    /// furthest above its fair share while memory is short.
    Share = 1,
    /// The machine as a whole is out of memory (kernel reserve reached); not this group's doing.
    Reserve = 2,
}

impl Denied {
    /// Inverse of `d as u64 + 1` (0 = none).
    pub fn from_code(code: u64) -> Option<Denied> {
        match code {
            1 => Some(Denied::Limit),
            2 => Some(Denied::Share),
            3 => Some(Denied::Reserve),
            _ => None,
        }
    }
}

/// Error of a group operation, with the Linux error number it maps to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupError {
    /// No such group.
    NotFound,
    /// A group of that name exists already.
    Exists,
    /// The group still has processes or child groups, or the new limit is below the current use.
    Busy,
    /// Bad name or value.
    Invalid,
    /// No free slot, or nesting too deep.
    NoSpace,
    /// A fixed branch cannot be removed.
    Fixed,
    /// The limit of the target group does not allow one more thread.
    Again,
}

impl GroupError {
    /// Negative Linux error number.
    pub fn errno(self) -> i64 {
        match self {
            GroupError::NotFound => -2,
            GroupError::Exists => -17,
            GroupError::Busy => -16,
            GroupError::Invalid => -22,
            GroupError::NoSpace => -28,
            GroupError::Fixed => -1,
            GroupError::Again => -11,
        }
    }
}

struct Slot {
    state: AtomicU8,
    /// Created by the kernel for a cell: disappears with its last process.
    auto: AtomicBool,
    parent: AtomicU16,
    mem: AtomicU64,
    mem_peak: AtomicU64,
    mem_max: AtomicU64,
    /// Threads in this group and all descendants.
    pids: AtomicU64,
    pids_peak: AtomicU64,
    pids_max: AtomicU64,
    /// Threads that are direct members.
    own: AtomicU64,
    /// Refusals by `memory.max`, kills for lack of memory, refusals by `pids.max`.
    events: [AtomicU64; 3],
    name: spin::Mutex<([u8; MAX_NAME], usize)>,
}

impl Slot {
    const fn new() -> Slot {
        Slot {
            state: AtomicU8::new(FREE),
            auto: AtomicBool::new(false),
            parent: AtomicU16::new(ROOT),
            mem: AtomicU64::new(0),
            mem_peak: AtomicU64::new(0),
            mem_max: AtomicU64::new(NO_LIMIT),
            pids: AtomicU64::new(0),
            pids_peak: AtomicU64::new(0),
            pids_max: AtomicU64::new(NO_LIMIT),
            own: AtomicU64::new(0),
            events: [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)],
            name: spin::Mutex::new(([0; MAX_NAME], 0)),
        }
    }
}

static SLOTS: [Slot; MAX_GROUPS] = [const { Slot::new() }; MAX_GROUPS];
/// Serialises changes of the tree's shape (create, remove).
static TREE: spin::Mutex<()> = spin::Mutex::new(());

fn slot(g: u16) -> &'static Slot {
    &SLOTS[(g as usize) % MAX_GROUPS]
}

fn parent(g: u16) -> u16 {
    slot(g).parent.load(Ordering::Relaxed)
}

/// The group and its ancestors up to the root, nearest first.
fn chain(g: u16) -> impl Iterator<Item = u16> {
    let mut cur = Some(g);
    let mut steps = 0;
    core::iter::from_fn(move || {
        let g = cur?;
        steps += 1;
        cur = (g != ROOT && steps <= MAX_DEPTH + 1).then(|| parent(g));
        Some(g)
    })
}

/// Set up the fixed branches and the dynamic rules for a machine with `total` bytes of memory.
pub fn init(total: u64) {
    for g in 0..FIXED {
        let s = slot(g);
        s.parent.store(ROOT, Ordering::Relaxed);
        let mut n = s.name.lock();
        let name = FIXED_NAMES[g as usize].as_bytes();
        n.0[..name.len()].copy_from_slice(name);
        n.1 = name.len();
        s.state.store(LIVE, Ordering::Relaxed);
    }
    policy::init(total);
}

/// Whether `g` names a group that exists (not removed).
pub fn is_live(g: u16) -> bool {
    (g as usize) < MAX_GROUPS && slot(g).state.load(Ordering::Relaxed) == LIVE
}

/// Branch (`SYSTEM` … `CELLS`) the group lies in; `ROOT` for the root itself.
pub fn branch_of(g: u16) -> u16 {
    chain(g).take_while(|&x| x != ROOT).last().unwrap_or(ROOT)
}

/// The group's ancestor directly below its branch (the unit the dynamic rules compare); a branch or
/// the root is its own top.
pub fn top_of(g: u16) -> u16 {
    let mut top = g;
    let mut cur = g;
    while cur != ROOT && parent(cur) != ROOT {
        top = cur;
        cur = parent(cur);
    }
    if cur == g {
        g
    } else {
        top
    }
}

/// Whether `g` is `ancestor` or lies below it.
pub fn within(g: u16, ancestor: u16) -> bool {
    chain(g).any(|x| x == ancestor)
}

// ------------------------------------------------------------------ memory

fn raise_peak(peak: &AtomicU64, now: u64) {
    if now > peak.load(Ordering::Relaxed) {
        peak.fetch_max(now, Ordering::Relaxed);
    }
}

/// Number of groups with an explicit `memory.max` (lets the common case skip the limit checks).
static LIMITS: AtomicU64 = AtomicU64::new(0);

/// Whether any group has an explicit `memory.max`.
pub fn limits_set() -> bool {
    LIMITS.load(Ordering::Relaxed) != 0
}

/// Charge `n` bytes to `g` and its ancestors without asking the limits.
pub fn charge_forced(g: u16, n: u64) {
    let mut cur = g;
    loop {
        let s = slot(cur);
        raise_peak(&s.mem_peak, s.mem.fetch_add(n, Ordering::Relaxed) + n);
        if cur == ROOT {
            break;
        }
        cur = s.parent.load(Ordering::Relaxed);
    }
}

fn take_back(g: u16, n: u64, levels: usize) {
    for x in chain(g).take(levels) {
        slot(x).mem.fetch_sub(n, Ordering::Relaxed);
    }
}

/// Charge `n` bytes to `g` and its ancestors if the explicit limits and the dynamic rules allow it.
pub fn try_charge(g: u16, n: u64) -> Result<(), Denied> {
    let mut done = 0;
    let mut cur = g;
    loop {
        let s = slot(cur);
        let max = s.mem_max.load(Ordering::Relaxed);
        let now = if max == NO_LIMIT {
            s.mem.fetch_add(n, Ordering::Relaxed) + n
        } else {
            match s
                .mem
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                    v.checked_add(n).filter(|&v| v <= max)
                }) {
                Ok(v) => v + n,
                Err(_) => {
                    s.events[0].fetch_add(1, Ordering::Relaxed);
                    take_back(g, n, done);
                    return Err(Denied::Limit);
                }
            }
        };
        raise_peak(&s.mem_peak, now);
        done += 1;
        if cur == ROOT {
            break;
        }
        cur = s.parent.load(Ordering::Relaxed);
    }
    if let Err(d) = policy::check(g, n) {
        take_back(g, n, done);
        return Err(d);
    }
    Ok(())
}

/// Release `n` bytes charged to `g`.
pub fn uncharge(g: u16, n: u64) {
    let mut cur = g;
    loop {
        let s = slot(cur);
        let up = s.parent.load(Ordering::Relaxed);
        // charges and releases are balanced; should a count ever fall below zero it stays at zero
        if s.mem.fetch_sub(n, Ordering::Relaxed) < n {
            s.mem.store(0, Ordering::Relaxed);
        }
        if s.state.load(Ordering::Relaxed) == DYING {
            release_if_done(cur);
        }
        if cur == ROOT {
            break;
        }
        cur = up;
    }
}

/// A removed group gives its slot back once nothing is charged to it any more.
fn release_if_done(g: u16) {
    let s = slot(g);
    if s.state.load(Ordering::Relaxed) == DYING
        && s.mem.load(Ordering::Relaxed) == 0
        && s.pids.load(Ordering::Relaxed) == 0
    {
        let _ = s
            .state
            .compare_exchange(DYING, FREE, Ordering::Relaxed, Ordering::Relaxed);
    }
}

/// Bytes charged to the group (with descendants).
pub fn memory(g: u16) -> u64 {
    slot(g).mem.load(Ordering::Relaxed)
}

/// Whether the group or an ancestor is above its explicit `memory.max` (small kernel objects are
/// charged without asking; the caller ends the process that pushed the group over the line).
pub fn over_limit(g: u16) -> bool {
    chain(g).any(|x| slot(x).mem.load(Ordering::Relaxed) > slot(x).mem_max.load(Ordering::Relaxed))
}

/// The allocators report that the kernel reserve itself ran out (emergency heap in use).
pub fn memory_critical() {
    policy::set_critical();
}

/// Explicit `memory.max` (`NO_LIMIT` = none).
pub fn memory_max(g: u16) -> u64 {
    slot(g).mem_max.load(Ordering::Relaxed)
}

/// Set `memory.max`. A value below what the group uses now is refused (`Busy`): the kernel has no
/// way to take memory back from running programs.
pub fn set_memory_max(g: u16, max: u64) -> Result<(), GroupError> {
    if !is_live(g) || g == ROOT {
        return Err(GroupError::Invalid);
    }
    if max < slot(g).mem.load(Ordering::Relaxed) {
        return Err(GroupError::Busy);
    }
    let old = slot(g).mem_max.swap(max, Ordering::Relaxed);
    match (old == NO_LIMIT, max == NO_LIMIT) {
        (true, false) => {
            LIMITS.fetch_add(1, Ordering::Relaxed);
        }
        (false, true) => {
            LIMITS.fetch_sub(1, Ordering::Relaxed);
        }
        _ => {}
    }
    Ok(())
}

/// Explicit `pids.max` (`NO_LIMIT` = none, the dynamic ceiling applies).
pub fn pids_max(g: u16) -> u64 {
    slot(g).pids_max.load(Ordering::Relaxed)
}

/// Set `pids.max`. Like Linux, a value below the current count is accepted: no new thread can be
/// created until the count has fallen below it.
pub fn set_pids_max(g: u16, max: u64) -> Result<(), GroupError> {
    if !is_live(g) || g == ROOT {
        return Err(GroupError::Invalid);
    }
    slot(g).pids_max.store(max, Ordering::Relaxed);
    Ok(())
}
