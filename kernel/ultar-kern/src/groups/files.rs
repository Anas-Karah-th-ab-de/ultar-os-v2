//! The group tree as files, in the layout of Linux' `cgroup2` at `/sys/fs/cgroup`, so that existing
//! tools (container managers, `systemd-cgls`-like viewers, shell scripts) can read and set limits.
//!
//! A directory per group; `mkdir`/`rmdir` create and remove groups. Files of every group:
//!
//! | File | Read | Write |
//! |---|---|---|
//! | `cgroup.procs` | process ids in the group (as the reader's pid namespace numbers them) | a pid (`0` = the writer) moves that process here |
//! | `cgroup.controllers`, `cgroup.events` | `memory pids`, `populated 0/1` | – |
//! | `memory.current`, `memory.peak` | bytes charged now / highest value | – |
//! | `memory.max` | explicit limit or `max` | bytes (suffix `K`/`M`/`G`) or `max` |
//! | `memory.min` | protected minimum of the dynamic rules (`system`, `session`) | – |
//! | `memory.events` | `max` (refusals by the limit), `oom_kill` | – |
//! | `pids.current`, `pids.peak` | threads now / highest value | – |
//! | `pids.max` | explicit limit or `max` | number or `max` |
//! | `pids.events` | `max` (refused forks) | – |
//! | `ultar.pids.effective` | ceiling in force: explicit or dynamic | – |
//!
//! Every change (write, `mkdir`, `rmdir`) needs the right `MANAGE_GROUPS`. Processes cannot be
//! moved into the root, `apps` or `cells` themselves – only into a group below (the dynamic rules
//! compare those groups). Memory already charged stays with the old group until it is freed, as
//! under Linux; the threads are counted in the new group at once.
//!
//! Errors of `mkdir`/`rmdir` reach the caller through the per-processor error slot (the file
//! layer only knows yes/no).

use super::{GroupError, APPS, CELLS, MAX_NAME, NO_LIMIT, ROOT, SESSION, SYSTEM};
use crate::fs::{DirEnt, Node};
use crate::isolation::rights::Rights;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Where the tree is found in the host's file tree.
pub const ROOT_PATH: &str = "/sys/fs/cgroup";
const FILES: [&str; 14] = [
    "cgroup.controllers",
    "cgroup.events",
    "cgroup.procs",
    "memory.current",
    "memory.events",
    "memory.max",
    "memory.min",
    "memory.peak",
    "pids.current",
    "pids.events",
    "pids.max",
    "pids.peak",
    "ultar.pids.effective",
    "ultar.memory.pressure",
];
const EPERM: i64 = 1;
const EACCES: i64 = 13;
const EBUSY: i64 = 16;
const EINVAL: i64 = 22;
const ESRCH: i64 = 3;

/// Whether the host path is the tree or one of the directories leading to it (`/sys`, `/sys/fs`).
pub fn is_tree(path: &str) -> bool {
    path == "/sys" || path == "/sys/fs" || is_inside(path)
}

/// Whether the host path is the tree's root or lies below it.
pub fn is_inside(path: &str) -> bool {
    path.strip_prefix(ROOT_PATH).is_some_and(|r| r.is_empty() || r.starts_with('/'))
}

/// Group a tree path names and, if the last component is not a group, that component (a file name).
fn resolve(path: &str) -> Option<(u16, Option<&str>)> {
    let rest = path.strip_prefix(ROOT_PATH)?;
    let mut g = ROOT;
    let mut parts = rest.split('/').filter(|c| !c.is_empty()).peekable();
    while let Some(c) = parts.next() {
        match super::child(g, c) {
            Some(next) => g = next,
            None if parts.peek().is_none() => return Some((g, Some(c))),
            None => return None,
        }
    }
    Some((g, None))
}

/// Path of group `g` below the tree's root, e.g. `/cells/cell-12` (`/` for the root).
pub fn path_of(g: u16) -> String {
    let mut names: Vec<String> = Vec::new();
    let mut cur = g;
    while cur != ROOT && names.len() <= super::MAX_DEPTH {
        let mut buf = [0u8; MAX_NAME];
        let n = super::name(cur, &mut buf);
        names.push(String::from_utf8_lossy(&buf[..n]).into_owned());
        cur = super::parent(cur);
    }
    let mut s = String::new();
    for n in names.iter().rev() {
        s.push('/');
        s.push_str(n);
    }
    if s.is_empty() {
        s.push('/');
    }
    s
}

fn limit_text(v: u64) -> String {
    if v == NO_LIMIT {
        String::from("max\n")
    } else {
        format!("{v}\n")
    }
}

fn file_text(g: u16, file: &str) -> Option<String> {
    let u = super::usage(g);
    Some(match file {
        "cgroup.controllers" => String::from("memory pids\n"),
        "cgroup.events" => format!("populated {}\n", (u.pids.0 > 0) as u8),
        "cgroup.procs" => {
            let mut s = String::new();
            for pid in crate::proc::group_members(g) {
                s.push_str(&format!("{pid}\n"));
            }
            s
        }
        "memory.current" => format!("{}\n", u.mem.0),
        "memory.peak" => format!("{}\n", u.mem.1),
        "memory.max" => limit_text(super::memory_max(g)),
        "memory.min" => format!("{}\n", super::policy::protected(g)),
        "memory.events" => format!("max {}\noom_kill {}\n", u.events[0], u.events[1]),
        "pids.current" => format!("{}\n", u.pids.0),
        "pids.peak" => format!("{}\n", u.pids.1),
        "pids.max" => limit_text(super::pids_max(g)),
        "pids.events" => format!("max {}\n", u.events[2]),
        "ultar.pids.effective" => format!("{}\n", super::pids_limit(g)),
        "ultar.memory.pressure" => format!(
            "free {}\nreserve {}\ncritical {}\nlow {}\n",
            crate::mem::free_bytes(),
            crate::mem::reserve_bytes(),
            super::policy::critical(),
            super::policy::low()
        ),
        _ => return None,
    })
}

/// Node at a path of the tree (or `/sys`, `/sys/fs`).
pub fn open(path: &str) -> Option<Node> {
    let dir = |name: &str| Node::Dir(alloc::vec![DirEnt { name: String::from(name), dir: true, size: 0 }]);
    match path {
        "/sys" => {
            let mut v = alloc::vec![DirEnt { name: String::from("fs"), dir: true, size: 0 }];
            v.extend(crate::devices::sysfs::TOP.iter().map(|n| DirEnt { name: String::from(*n), dir: true, size: 0 }));
            return Some(Node::Dir(v));
        }
        "/sys/fs" => return Some(dir("cgroup")),
        _ => {}
    }
    match resolve(path)? {
        (g, Some(file)) => file_text(g, file).map(|s| Node::File(Arc::new(s.into_bytes()))),
        (g, None) => {
            let mut v: Vec<DirEnt> = FILES.iter().map(|n| DirEnt { name: String::from(*n), dir: false, size: 0 }).collect();
            super::for_children(g, |c| {
                let mut buf = [0u8; MAX_NAME];
                let n = super::name(c, &mut buf);
                v.push(DirEnt { name: String::from_utf8_lossy(&buf[..n]).into_owned(), dir: true, size: 0 });
            });
            Some(Node::Dir(v))
        }
    }
}

fn fail(errno: i64) -> bool {
    crate::percpu::me().errno = errno as u64;
    false
}

fn may_manage() -> bool {
    // the kernel console (no process) may; a process needs the right
    crate::proc::current() == 0 || crate::isolation::calls::holds(Rights::MANAGE_GROUPS)
}

/// `mkdir` in the tree: create a group. On `false` the error slot holds the reason.
pub fn mkdir(path: &str) -> bool {
    if !may_manage() {
        return fail(EPERM);
    }
    match resolve(path) {
        Some((parent, Some(name))) => match crate::heap::kernel(|| super::create(parent, name, false)) {
            Ok(_) => true,
            Err(e) => fail(-e.errno()),
        },
        Some((_, None)) => fail(17),
        None => fail(2),
    }
}

/// `rmdir`/`unlink` in the tree: remove a group. On `false` the error slot holds the reason.
pub fn rmdir(path: &str) -> bool {
    if !may_manage() {
        return fail(EPERM);
    }
    match resolve(path) {
        Some((g, None)) => match super::remove(g) {
            Ok(()) => true,
            Err(GroupError::Fixed) => fail(EBUSY),
            Err(e) => fail(-e.errno()),
        },
        Some((g, Some(file))) if file_text(g, file).is_some() => fail(EPERM),
        _ => fail(2),
    }
}

fn parse_limit(text: &str, suffixes: bool) -> Option<u64> {
    if text == "max" {
        return Some(NO_LIMIT);
    }
    let (digits, unit) = match text.as_bytes().last().map(|b| b.to_ascii_uppercase()) {
        Some(b'K') if suffixes => (&text[..text.len() - 1], 1u64 << 10),
        Some(b'M') if suffixes => (&text[..text.len() - 1], 1 << 20),
        Some(b'G') if suffixes => (&text[..text.len() - 1], 1 << 30),
        _ => (text, 1),
    };
    digits.parse::<u64>().ok()?.checked_mul(unit).filter(|&v| v != NO_LIMIT)
}

/// A write to a file of the tree. The caller has checked the right `MANAGE_GROUPS`.
/// `pid_of`: global number of a pid named in the writer's pid namespace.
/// `Err`: negative error number.
pub fn write(path: &str, text: &str, pid_of: &dyn Fn(u64) -> Option<u64>) -> Result<(), i64> {
    let (g, file) = match resolve(path) {
        Some((g, Some(f))) => (g, f),
        _ => return Err(-EINVAL),
    };
    let done = |r: Result<(), GroupError>| r.map_err(|e| e.errno());
    match file {
        "memory.max" => done(super::set_memory_max(g, parse_limit(text, true).ok_or(-EINVAL)?)),
        "pids.max" => done(super::set_pids_max(g, parse_limit(text, false).ok_or(-EINVAL)?)),
        "cgroup.procs" => {
            // the units of the dynamic rules are the groups below `apps` and `cells`: no process
            // lives in those two or in the root itself
            if matches!(g, ROOT | APPS | CELLS) {
                return Err(-EBUSY);
            }
            let local: u64 = text.parse().map_err(|_| -EINVAL)?;
            let tgid = if local == 0 { crate::proc::current_group() } else { pid_of(local).ok_or(-ESRCH)? };
            crate::proc::move_group(tgid, g)
        }
        f if file_text(g, f).is_some() => Err(-EACCES),
        _ => Err(-EINVAL),
    }
}

/// The whole tree with the dynamic rules as text (`/proc/ultar/gruppen`, console command `gruppen`).
pub fn table() -> String {
    let mib = |v: u64| v >> 10;
    let (lingering, lingering_bytes) = super::lingering();
    let mut s = format!(
        "speicher gesamt {} KiB frei {} KiB\nreserve {} KiB kritisch {} KiB knapp {} KiB\nschutz system {} KiB session {} KiB\nnotvorrat benutzt {} beendet {} fehlfreigaben {}\nreste {} gruppen {} bytes\n",
        mib(crate::mem::total_bytes()),
        mib(crate::mem::free_bytes()),
        mib(crate::mem::reserve_bytes()),
        mib(super::policy::critical()),
        mib(super::policy::low()),
        mib(super::policy::protected(SYSTEM)),
        mib(super::policy::protected(SESSION)),
        crate::heap::emergency_uses(),
        crate::oom::kills(),
        crate::mem::bad_frees(),
        lingering,
        lingering_bytes,
    );
    s.push_str("gruppe speicher grenze spitze prozesse grenze\n");
    fn row(g: u16, s: &mut String) {
        let u = super::usage(g);
        let max = super::memory_max(g);
        s.push_str(&format!(
            "{} {} {} {} {} {}\n",
            path_of(g),
            u.mem.0,
            if max == NO_LIMIT { String::from("max") } else { format!("{max}") },
            u.mem.1,
            u.pids.0,
            super::pids_limit(g)
        ));
        let mut kids = Vec::new();
        super::for_children(g, |c| kids.push(c));
        for c in kids {
            row(c, s);
        }
    }
    row(ROOT, &mut s);
    s
}
