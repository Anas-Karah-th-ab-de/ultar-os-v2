//! `waitid(idtype, id, infop, options)` (247): which child to wait for (`P_ALL`, `P_PID`, `P_PGID`),
//! what to wait for (`WEXITED`; `WSTOPPED`/`WCONTINUED` are accepted – the kernel has no stopped
//! children), `WNOHANG`, and `WNOWAIT` (the child stays a zombie). Wine and Python wait this way.

use crate::proc::{Proc, State, PROCS};
use crate::syscall::{put, with};

const ECHILD: i64 = 10;
const EFAULT: i64 = 14;
const EINVAL: i64 = 22;

const P_ALL: u64 = 0;
const P_PID: u64 = 1;
const P_PGID: u64 = 2;

const WNOHANG: u64 = 1;
const WSTOPPED: u64 = 2;
const WEXITED: u64 = 4;
const WCONTINUED: u64 = 8;
const WNOWAIT: u64 = 0x0100_0000;
/// `__WNOTHREAD`, `__WALL`, `__WCLONE`: accepted (every child is a thread group of its own here).
const WTHREAD_BITS: u64 = 0xe000_0000;

const SIGCHLD: i32 = 17;
const CLD_EXITED: i32 = 1;
const CLD_KILLED: i32 = 2;
const CLD_DUMPED: i32 = 3;

/// What the call does.
pub enum Outcome {
    /// Finished with this value.
    Val(i64),
    /// Wait until a child changes (the argument as `Wait::Child` takes it), then run the call again.
    Block(i64),
}

/// Which children a call names: one (global pid), a process group (global pgid) or all.
#[derive(Clone, Copy)]
enum Target {
    All,
    Pid(u64),
    Group(u64),
}

impl Target {
    fn matches(self, c: &Proc) -> bool {
        match self {
            Target::All => true,
            Target::Pid(p) => c.pid == p,
            Target::Group(g) => c.pgid == g,
        }
    }

    /// The argument of `Wait::Child`: one child, or any child (re-checked when the call runs again).
    fn wait_arg(self) -> i64 {
        match self {
            Target::Pid(p) => p as i64,
            _ => 0,
        }
    }
}

pub fn waitid(idtype: u64, id: u64, info: u64, options: u64) -> Outcome {
    if options & !(WNOHANG | WSTOPPED | WEXITED | WCONTINUED | WNOWAIT | WTHREAD_BITS) != 0 || options & (WSTOPPED | WEXITED | WCONTINUED) == 0 {
        return Outcome::Val(-EINVAL);
    }
    let Some((me, my_pgid, ns)) = with(|p| (p.tgid, p.pgid, p.link.ns().clone())) else { return Outcome::Val(-ECHILD) };
    // the caller names children and groups by the numbers of its own pid namespace
    let target = match idtype {
        P_ALL => Target::All,
        P_PID => match ns.global(id) {
            Some(g) if id != 0 => Target::Pid(g),
            _ => return Outcome::Val(-ECHILD),
        },
        P_PGID if id == 0 => Target::Group(my_pgid),
        P_PGID => match ns.global(id) {
            Some(g) => Target::Group(g),
            None => return Outcome::Val(-ECHILD),
        },
        _ => return Outcome::Val(-EINVAL),
    };
    let mut ps = PROCS.lock();
    let mine = |c: &&alloc::boxed::Box<Proc>| c.pid == c.tgid && c.ppid == me && target.matches(c);
    if !ps.iter().any(|c| mine(&c)) {
        return Outcome::Val(-ECHILD);
    }
    let ended = ps.iter().position(|c| mine(&c) && matches!(c.state, State::Zombie(_)) && !c.on_cpu);
    if let (Some(i), true) = (ended, options & WEXITED != 0) {
        let State::Zombie(st) = ps[i].state else { unreachable!() };
        let local = ns.local(ps[i].pid).unwrap_or(0);
        if options & WNOWAIT == 0 {
            ps.remove(i);
        }
        drop(ps);
        return Outcome::Val(report(info, local, st));
    }
    drop(ps);
    if options & WNOHANG != 0 {
        // nothing to report: si_signo and si_pid stay 0 (glibc and Python test si_pid)
        return Outcome::Val(if info == 0 || put(info, &[0u8; 32]) { 0 } else { -EFAULT });
    }
    Outcome::Block(target.wait_arg())
}

/// Fill the siginfo of an ended child (`st` as `wait4` reports it).
fn report(info: u64, pid: u64, st: i32) -> i64 {
    if info == 0 {
        return 0;
    }
    let (code, status) = if st & 0x7f == 0 {
        (CLD_EXITED, (st >> 8) & 0xff)
    } else if st & 0x80 != 0 {
        (CLD_DUMPED, st & 0x7f)
    } else {
        (CLD_KILLED, st & 0x7f)
    };
    let mut b = [0u8; 128];
    b[0..4].copy_from_slice(&SIGCHLD.to_le_bytes());
    b[8..12].copy_from_slice(&code.to_le_bytes());
    b[16..20].copy_from_slice(&(pid as i32).to_le_bytes());
    b[24..28].copy_from_slice(&status.to_le_bytes());
    if put(info, &b) {
        0
    } else {
        -EFAULT
    }
}
