//! Who may signal whom: pid namespace visibility, the pid 1 rule and the `KILL_ANY` right.

use super::rights::Rights;
use crate::proc::{Proc, State};
use alloc::boxed::Box;
use alloc::vec::Vec;

const SIGKILL: u64 = 9;
const SIGSTOP: u64 = 19;
const ESRCH: i64 = 3;
const EPERM: i64 = 1;

/// What happens to one signal from a sender to a target group.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Deliver it.
    Deliver,
    /// Report success but drop it (pid 1 of a namespace is shielded from its own namespace).
    Ignore,
    /// The sender lacks the right: `EPERM`.
    Denied,
    /// The sender cannot see the target: `ESRCH`.
    Invisible,
}

fn leader(ps: &[Box<Proc>], tgid: u64) -> Option<&Proc> {
    ps.iter().find(|p| p.pid == tgid).map(|b| &**b)
}

/// Whether group `tgid` descends from group `ancestor` (parent chain, bounded).
pub fn is_descendant(ps: &[Box<Proc>], ancestor: u64, tgid: u64) -> bool {
    let mut cur = tgid;
    for _ in 0..64 {
        let Some(p) = leader(ps, cur) else { return false };
        if p.ppid == 0 {
            return false;
        }
        if p.ppid == ancestor {
            return true;
        }
        cur = p.ppid;
    }
    false
}

/// Decide a signal `sig` from the thread at index `sender` to the group `target` (global number).
pub fn verdict(ps: &[Box<Proc>], sender: usize, target: u64, sig: u64) -> Verdict {
    let s = &ps[sender];
    let Some(t) = leader(ps, target) else { return Verdict::Invisible };
    if s.link.ns().local(target).is_none() {
        return Verdict::Invisible;
    }
    if target != s.tgid && !s.g.iso.rights.has(Rights::KILL_ANY) && !is_descendant(ps, s.tgid, target) {
        return Verdict::Denied;
    }
    let tns = t.link.ns();
    if !tns.is_initial() && tns.init_global() == target && tns.contains(s.link.ns()) {
        // pid 1 of a namespace, signalled from inside: only signals it handles reach it
        let handled = sig != 0 && sig <= 64 && t.g.sigact[sig as usize].handler > 1;
        if sig == SIGKILL || sig == SIGSTOP || !handled {
            return Verdict::Ignore;
        }
    }
    Verdict::Deliver
}

/// Groups (global numbers) a `kill(pid, ..)` of the thread at index `sender` addresses, before the
/// permission check. `Err` carries the negative error number.
pub fn kill_targets(ps: &[Box<Proc>], sender: usize, pid: i64) -> Result<Vec<u64>, i64> {
    let s = &ps[sender];
    let ns = s.link.ns();
    let visible = |p: &&Box<Proc>| p.pid == p.tgid && ns.local(p.tgid).is_some();
    let alive = |t: &u64| ps.iter().any(|p| p.tgid == *t && !matches!(p.state, State::Zombie(_) | State::Dead));
    let all: Vec<u64> = match pid {
        p if p > 0 => ns.global(p as u64).into_iter().collect(),
        0 => ps.iter().filter(visible).filter(|p| p.pgid == s.pgid).map(|p| p.tgid).collect(),
        // never the sender itself and never pid 1 of the sender's own (or an enclosing) namespace
        -1 => ps.iter().filter(visible).filter(|p| p.tgid != s.tgid && !(p.link.ns().init_global() == p.tgid && p.link.ns().contains(ns))).map(|p| p.tgid).collect(),
        p => {
            let Some(pg) = ns.global(p.unsigned_abs()) else { return Err(-ESRCH) };
            ps.iter().filter(visible).filter(|x| x.pgid == pg).map(|x| x.tgid).collect()
        }
    };
    let live: Vec<u64> = all.into_iter().filter(alive).collect();
    if live.is_empty() {
        return Err(-ESRCH);
    }
    Ok(live)
}

/// Apply the verdicts to a target list: the groups to really signal, or the error for the caller.
/// A broadcast (`single == false`) skips targets the sender may not signal and fails only if none is left.
pub fn permitted(ps: &[Box<Proc>], sender: usize, targets: &[u64], sig: u64, single: bool) -> Result<Vec<u64>, i64> {
    let mut out = Vec::new();
    let mut allowed = 0;
    for &t in targets {
        match verdict(ps, sender, t, sig) {
            Verdict::Deliver => {
                allowed += 1;
                out.push(t);
            }
            Verdict::Ignore => allowed += 1,
            Verdict::Denied if single => return Err(-EPERM),
            Verdict::Invisible if single => return Err(-ESRCH),
            Verdict::Denied | Verdict::Invisible => {}
        }
    }
    if allowed == 0 {
        return Err(-EPERM);
    }
    Ok(out)
}
