//! `flock(2)` – advisory whole-file locks, as the system updater uses them to keep a second
//! instance off its state directory.
//!
//! A lock belongs to the thread group (process) that took it and is keyed by the file's path in
//! the kernel's tree. It ends with `LOCK_UN`, or when the holding process has ended (checked
//! whenever someone else asks for the lock). Deviation from Linux: closing one descriptor does
//! not drop the lock while the process lives; shared locks (`LOCK_SH`) are taken as exclusive.

use crate::proc::{FdKind, State, PROCS};
use crate::syscall::with;
use alloc::collections::BTreeMap;
use alloc::string::String;
use spin::Mutex;

const EBADF: i64 = 9;
const EINVAL: i64 = 22;
const EWOULDBLOCK: i64 = 11;
const ENOLCK: i64 = 37;
const LOCK_SH: u64 = 1;
const LOCK_EX: u64 = 2;
const LOCK_NB: u64 = 4;
const LOCK_UN: u64 = 8;
/// Upper bound of locks held at once (each entry is one path).
const MAX_LOCKS: usize = 1024;

static LOCKS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());

fn alive(tgid: u64) -> bool {
    PROCS.lock().iter().any(|p| p.tgid == tgid && !matches!(p.state, State::Zombie(_) | State::Exiting | State::Dead))
}

/// The `flock` system call. A blocking request on a held lock is answered like `LOCK_NB`
/// (`EWOULDBLOCK`): the kernel has no wait queue for it, and the caller decides how to retry.
pub fn sys_flock(fd: u64, op: u64) -> i64 {
    let target = with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(FdKind::File { path, .. }) | Some(FdKind::Dir { path, .. }) => Ok((path.clone(), p.tgid)),
        Some(_) => Err(-EINVAL),
        None => Err(-EBADF),
    });
    let (path, me) = match target {
        Some(Ok(t)) => t,
        Some(Err(e)) => return e,
        None => return -EBADF,
    };
    let mut locks = LOCKS.lock();
    match op & !LOCK_NB {
        LOCK_UN => {
            if locks.get(&path) == Some(&me) {
                locks.remove(&path);
            }
            0
        }
        LOCK_SH | LOCK_EX => match locks.get(&path).copied() {
            Some(holder) if holder == me => 0,
            Some(holder) if alive(holder) => -EWOULDBLOCK,
            held => {
                if held.is_none() && locks.len() >= MAX_LOCKS {
                    locks.retain(|_, h| alive(*h));
                    if locks.len() >= MAX_LOCKS {
                        return -ENOLCK;
                    }
                }
                locks.insert(path, me);
                0
            }
        },
        _ => -EINVAL,
    }
}
