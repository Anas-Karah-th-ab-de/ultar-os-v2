//! Shape of the group tree: names, creating and removing groups, the automatic group of a cell.

use super::{
    branch_of, chain, is_live, parent, release_if_done, slot, GroupError, CELLS, DYING, FIXED,
    FREE, LIMITS, LIVE, MAX_DEPTH, MAX_GROUPS, MAX_NAME, NO_LIMIT, ROOT, SESSION, SYSTEM, TREE,
};
use core::sync::atomic::Ordering;

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn name_is(g: u16, name: &str) -> bool {
    let n = slot(g).name.lock();
    &n.0[..n.1] == name.as_bytes()
}

/// Copy the group's name into `out`; returns its length.
pub fn name(g: u16, out: &mut [u8; MAX_NAME]) -> usize {
    let n = slot(g).name.lock();
    *out = n.0;
    n.1
}

/// Live child of `parent_group` called `name`.
pub fn child(parent_group: u16, name: &str) -> Option<u16> {
    (1..MAX_GROUPS as u16)
        .find(|&g| is_live(g) && parent(g) == parent_group && g != ROOT && name_is(g, name))
}

/// Live children of `g`, in slot order; calls `f` for each.
pub fn for_children(g: u16, mut f: impl FnMut(u16)) {
    for c in 1..MAX_GROUPS as u16 {
        if is_live(c) && parent(c) == g {
            f(c);
        }
    }
}

fn depth(g: u16) -> usize {
    chain(g).count() - 1
}

/// Create a child group. `auto`: the kernel removes it when its last thread is gone.
pub fn create(parent_group: u16, name: &str, auto: bool) -> Result<u16, GroupError> {
    if !valid_name(name) {
        return Err(GroupError::Invalid);
    }
    let _tree = TREE.lock();
    if !is_live(parent_group) {
        return Err(GroupError::NotFound);
    }
    if depth(parent_group) >= MAX_DEPTH {
        return Err(GroupError::NoSpace);
    }
    if child(parent_group, name).is_some() {
        return Err(GroupError::Exists);
    }
    let g = (FIXED..MAX_GROUPS as u16)
        .find(|&g| slot(g).state.load(Ordering::Relaxed) == FREE)
        .ok_or(GroupError::NoSpace)?;
    let s = slot(g);
    s.auto.store(auto, Ordering::Relaxed);
    s.parent.store(parent_group, Ordering::Relaxed);
    for a in [
        &s.mem,
        &s.mem_peak,
        &s.pids,
        &s.pids_peak,
        &s.own,
        &s.events[0],
        &s.events[1],
        &s.events[2],
    ] {
        a.store(0, Ordering::Relaxed);
    }
    s.mem_max.store(NO_LIMIT, Ordering::Relaxed);
    s.pids_max.store(NO_LIMIT, Ordering::Relaxed);
    {
        let mut n = s.name.lock();
        n.0 = [0; MAX_NAME];
        n.0[..name.len()].copy_from_slice(name.as_bytes());
        n.1 = name.len();
    }
    s.state.store(LIVE, Ordering::Relaxed);
    Ok(g)
}

/// Remove a group without threads and without child groups. Memory still charged to it (objects
/// that outlive its processes) stays counted in the ancestors; the slot is reused once it is gone.
pub fn remove(g: u16) -> Result<(), GroupError> {
    if g < FIXED {
        return Err(GroupError::Fixed);
    }
    let _tree = TREE.lock();
    if !is_live(g) {
        return Err(GroupError::NotFound);
    }
    let mut children = false;
    for_children(g, |_| children = true);
    if children || slot(g).pids.load(Ordering::Relaxed) != 0 {
        return Err(GroupError::Busy);
    }
    if slot(g).mem_max.swap(NO_LIMIT, Ordering::Relaxed) != NO_LIMIT {
        LIMITS.fetch_sub(1, Ordering::Relaxed);
    }
    slot(g).state.store(DYING, Ordering::Relaxed);
    release_if_done(g);
    Ok(())
}

/// Group for a new cell started by a process of group `of`: a child of `cells` when the starter
/// belongs to the trusted branches (`system`, `session`, the kernel), otherwise a child of the
/// starter's own group – a process can never leave the limits of its group by starting a cell.
pub fn create_cell(of: u16, pid: u64) -> Result<u16, GroupError> {
    let under =
        if matches!(of, ROOT | SYSTEM | SESSION) || matches!(branch_of(of), SYSTEM | SESSION) {
            CELLS
        } else {
            of
        };
    let mut buf = [0u8; 24];
    let mut i = buf.len();
    let mut v = pid;
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let prefix = b"cell-";
    let start = i - prefix.len();
    buf[start..i].copy_from_slice(prefix);
    let name = core::str::from_utf8(&buf[start..]).map_err(|_| GroupError::Invalid)?;
    create(under, name, true)
}
