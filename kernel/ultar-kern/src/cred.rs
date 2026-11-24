//! User and group ids of a process, Linux-compatible: real, effective and saved user id and group
//! id, and supplementary groups. Inherited by `fork` and kept across `execve` (set-id bits of files
//! are not honoured). Programs started by the kernel are root (all ids 0), as before.
//!
//! Ids only *name* a process – to the file permission checks (`perm.rs`) and to the peer of a Unix
//! socket (`SO_PEERCRED`). What a process may do beyond its files is decided by its rights
//! (`isolation/rights.rs`): changing ids freely needs the right `SET_ID`; without it a process can
//! only move between the ids it already has, exactly as an unprivileged process on Linux.

use crate::isolation::rights::Rights;
use crate::syscall::with;

/// Most supplementary groups of one process.
pub const MAX_GROUPS: usize = 32;
/// "Leave unchanged" in `setreuid`/`setresuid` and never a valid id.
const KEEP: u32 = u32::MAX;
const EPERM: i64 = 1;
const EFAULT: i64 = 14;
const EINVAL: i64 = 22;

/// The ids of one thread group.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cred {
    /// Real, effective, saved user id.
    pub uid: [u32; 3],
    /// Real, effective, saved group id.
    pub gid: [u32; 3],
    groups: [u32; MAX_GROUPS],
    ngroups: usize,
}

impl Cred {
    /// Root: what every program started by the kernel has.
    pub const ROOT: Cred = Cred {
        uid: [0; 3],
        gid: [0; 3],
        groups: [0; MAX_GROUPS],
        ngroups: 0,
    };

    /// Effective user id.
    pub fn euid(&self) -> u32 {
        self.uid[1]
    }

    /// Effective group id.
    pub fn egid(&self) -> u32 {
        self.gid[1]
    }

    /// Supplementary groups.
    pub fn groups(&self) -> &[u32] {
        &self.groups[..self.ngroups]
    }

    /// Whether `gid` is the effective group or one of the supplementary groups.
    pub fn in_group(&self, gid: u32) -> bool {
        self.egid() == gid || self.groups().contains(&gid)
    }
}

/// `set*id` with one argument: privileged sets all three, otherwise only the effective id, to the
/// real or saved one.
fn set_one(ids: &mut [u32; 3], id: u32, privileged: bool) -> i64 {
    if id == KEEP {
        return -EINVAL;
    }
    if privileged {
        *ids = [id; 3];
    } else if id == ids[0] || id == ids[2] {
        ids[1] = id;
    } else {
        return -EPERM;
    }
    0
}

/// `setre*id`: unprivileged, the real id may become the real or effective one, the effective id any
/// of the three. The saved id follows the effective one when the real id is set or the effective id
/// becomes something other than the old real id.
fn set_two(ids: &mut [u32; 3], real: u32, effective: u32, privileged: bool) -> i64 {
    let old = *ids;
    let real_ok = real == KEEP || privileged || real == old[0] || real == old[1];
    let effective_ok = effective == KEEP || privileged || old.contains(&effective);
    if !real_ok || !effective_ok {
        return -EPERM;
    }
    if real != KEEP {
        ids[0] = real;
    }
    if effective != KEEP {
        ids[1] = effective;
    }
    if real != KEEP || effective != KEEP && effective != old[0] {
        ids[2] = ids[1];
    }
    0
}

/// `setres*id`: unprivileged, each new id must be one of the three current ones.
fn set_three(ids: &mut [u32; 3], new: [u32; 3], privileged: bool) -> i64 {
    let old = *ids;
    if !privileged && new.iter().any(|&n| n != KEEP && !old.contains(&n)) {
        return -EPERM;
    }
    for (slot, n) in ids.iter_mut().zip(new) {
        if n != KEEP {
            *slot = n;
        }
    }
    0
}

/// Run `f` on the caller's ids; the flag says whether it holds the right `SET_ID`.
fn change(f: impl FnOnce(&mut Cred, bool) -> i64) -> i64 {
    with(|p| {
        let privileged = p.g.iso.rights.has(Rights::SET_ID);
        f(&mut p.g.iso.cred, privileged)
    })
    .unwrap_or(-EPERM)
}

/// Ids of the calling process (root outside a process: the kernel console).
pub fn current() -> Cred {
    with(|p| p.g.iso.cred).unwrap_or(Cred::ROOT)
}

fn put_ids(ptrs: [u64; 3], ids: [u32; 3]) -> i64 {
    let ok = with(|p| {
        ptrs.iter().zip(ids).all(|(&ptr, id)| {
            ptr == 0 || crate::proc::write_user(p.space(), ptr, &id.to_le_bytes())
        })
    })
    .unwrap_or(false);
    if ok {
        0
    } else {
        -EFAULT
    }
}

fn get_groups(size: u64, ptr: u64) -> i64 {
    let c = current();
    let n = c.groups().len();
    if size == 0 {
        return n as i64;
    }
    if (size as usize) < n {
        return -EINVAL;
    }
    let ok = with(|p| {
        c.groups()
            .iter()
            .enumerate()
            .all(|(i, g)| crate::proc::write_user(p.space(), ptr + 4 * i as u64, &g.to_le_bytes()))
    })
    .unwrap_or(false);
    if ok {
        n as i64
    } else {
        -EFAULT
    }
}

fn set_groups(size: u64, ptr: u64) -> i64 {
    let n = size as usize;
    if size > MAX_GROUPS as u64 {
        return -EINVAL;
    }
    let Some(raw) = with(|p| crate::proc::read_user(p.space(), ptr, n * 4)).flatten() else {
        return -EFAULT;
    };
    change(|c, privileged| {
        if !privileged {
            return -EPERM;
        }
        for (slot, b) in c.groups.iter_mut().zip(raw.chunks_exact(4)) {
            *slot = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        }
        c.ngroups = n;
        0
    })
}

/// The id system calls. `None`: `nr` is not one of them.
pub fn call(nr: u64, a: [u64; 6]) -> Option<i64> {
    let (x, y, z) = (a[0] as u32, a[1] as u32, a[2] as u32);
    let c = current();
    Some(match nr {
        102 => c.uid[0] as i64,
        107 => c.uid[1] as i64,
        104 => c.gid[0] as i64,
        108 => c.gid[1] as i64,
        105 => change(|c, p| set_one(&mut c.uid, x, p)),
        106 => change(|c, p| set_one(&mut c.gid, x, p)),
        113 => change(|c, p| set_two(&mut c.uid, x, y, p)),
        114 => change(|c, p| set_two(&mut c.gid, x, y, p)),
        117 => change(|c, p| set_three(&mut c.uid, [x, y, z], p)),
        119 => change(|c, p| set_three(&mut c.gid, [x, y, z], p)),
        118 => put_ids([a[0], a[1], a[2]], c.uid),
        120 => put_ids([a[0], a[1], a[2]], c.gid),
        115 => get_groups(a[0] & 0xffff_ffff, a[1]),
        116 => set_groups(a[0], a[1]),
        // setfsuid / setfsgid: there is no separate file-system id; the call reports the effective
        // id (what Linux returns as "previous value") and changes nothing
        122 => c.uid[1] as i64,
        123 => c.gid[1] as i64,
        _ => return None,
    })
}
