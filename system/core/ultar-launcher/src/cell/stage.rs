//! The two helper stages and the platform probe (see the module above).
//! These functions run in fresh, single-threaded helper processes.
//!
//! Signals (review L-5): the launcher signals **only stage 1**, its own
//! child – whose number cannot be given to another process before the
//! launcher has reaped it, which it does only after it stopped signalling.
//! Stage 1 passes them on to stage 2 (its own child, same rule), stage 2 to
//! the app (its own child):
//!
//! | to stage 1 | stage 1 does | stage 2 does |
//! |---|---|---|
//! | `SIGTERM` | `SIGTERM` to stage 2 | `SIGTERM` to the app |
//! | `SIGUSR1` (end the cell) | `SIGKILL` to stage 2 (pid 1: the whole cell); without a pid namespace `SIGUSR1` | `SIGKILL` to the app |
//!
//! Both stages block these signals before anything else, so a signal that
//! comes early waits instead of ending a stage half way. Each stage dies
//! with its parent where the kernel offers that (`PR_SET_PDEATHSIG`).

use super::{view, CellFailure, CellPlan, StatusLine, CELL_INIT_ARG, STATUS_ENV};
use crate::platform::{
    PROBE_BASE, PROBE_CELL, PROBE_FILE_BIND, PROBE_IPC, PROBE_NET, PROBE_NET_GUARD,
};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command};
use ultar_sys::cell::{self as sys_cell, BindMode, Ended, Namespaces, StopSignal};
use ultar_sys::{Errno, Identity, Resource};

/// Highest descriptor number a stage closes at its start.
const CLOSE_UPPER: i32 = 1024;
/// `SIGUSR1`: end the cell.
const SIGUSR1: i32 = 10;
/// `SIGTERM`: passed on.
const SIGTERM: i32 = 15;
/// `SIGCHLD`: a child ended.
const SIGCHLD: i32 = 17;
/// The signals the stages handle.
const MASK: u64 = 1 << (SIGUSR1 - 1) | 1 << (SIGTERM - 1) | 1 << (SIGCHLD - 1);
/// Size of one `signalfd_siginfo` record.
const SIGINFO: usize = 128;
/// Exit codes of stage 2 above this are `128 + signal` of the app (the
/// shell's convention; an app's own exit code 129–192 reads as a signal).
const SIGNALLED: i32 = 128;

/// The status pipe of this stage, taken over first thing (every other
/// inherited descriptor above the standard streams is closed).
fn take_status() -> Option<File> {
    let number: i32 = std::env::var(STATUS_ENV).ok()?.parse().ok()?;
    let mut inherited = ultar_sys::take_inherited(&[number], true, CLOSE_UPPER).ok()?;
    inherited.take(number).map(File::from)
}

fn report(status: &mut File, line: StatusLine) {
    // A launcher that went away cannot be told; the stage goes on.
    let _ = status.write_all(format!("{}\n", line.render()).as_bytes());
}

fn failed(status: &mut File, failure: CellFailure) -> i32 {
    report(status, StatusLine::Fail(failure));
    failure as i32
}

/// What a stage's signal loop is told to do with a signal.
enum Relay {
    /// Send this to the child.
    Send(StopSignal),
    /// Nothing (a child ended: look at the children).
    Reap,
}

/// Waits for `child` to end, passing signals on after `relay`. Returns
/// how it ended. Any other child is reaped and forgotten.
fn relay_until_ended(child: &Child, relay: impl Fn(i32) -> Relay) -> Result<Ended, CellFailure> {
    let mut signals = File::from(ultar_sys::signal_fd(MASK).map_err(|_| CellFailure::Setup)?);
    let pid = child.id();
    let mut info = [0u8; SIGINFO];
    loop {
        // also before the first wait: the child may have ended already
        while let Ok(Some((reaped, ended))) = sys_cell::reap_any() {
            if reaped == pid {
                return Ok(ended);
            }
        }
        signals
            .read_exact(&mut info)
            .map_err(|_| CellFailure::Setup)?;
        let signal = i32::from_le_bytes([info[0], info[1], info[2], info[3]]);
        if let Relay::Send(what) = relay(signal) {
            let _ = sys_cell::send_signal(pid, what);
        }
    }
}

/// This program (stage 2 is the same binary); where the kernel cannot name
/// it, the path it was started by.
fn this_program() -> std::io::Result<std::path::PathBuf> {
    std::env::current_exe().or_else(|_| {
        std::env::args_os()
            .next()
            .map(std::path::PathBuf::from)
            .ok_or(std::io::ErrorKind::NotFound.into())
    })
}

/// Group and namespaces of stage 1.
fn enter(plan: &CellPlan) -> Result<(), CellFailure> {
    if let Some(dir) = &plan.group_dir {
        std::fs::write(dir.join("cgroup.procs"), std::process::id().to_string())
            .map_err(|_| CellFailure::Group)?;
    }
    if plan.namespaces != Namespaces::default() && sys_cell::unshare(plan.namespaces).is_err() {
        return Err(CellFailure::Namespaces);
    }
    if plan.namespaces.mount && sys_cell::make_mounts_private().is_err() {
        return Err(CellFailure::Mounts);
    }
    // the "internet only" barrier: set in stage 1, so stage 2 and the app inherit it and cannot drop it
    if plan.net_guard && !matches!(set_net_guard(), Ok(true)) {
        return Err(CellFailure::Namespaces);
    }
    Ok(())
}

/// Sets the barrier and reads it back.
fn set_net_guard() -> Result<bool, Errno> {
    ultar_sys::ultar_net_guard()?;
    ultar_sys::ultar_net_guarded()
}

/// Stage 1: group, namespaces, stage 2, then pass signals on until stage 2
/// ended. Returns the exit code.
pub fn stage_one(words: &[String]) -> i32 {
    let Some(mut status) = take_status() else {
        return CellFailure::Arguments as i32;
    };
    if ultar_sys::block_signals(MASK).is_err() {
        return failed(&mut status, CellFailure::Setup);
    }
    // ends with the launcher (Linux): stage 2 then ends with it
    let _ = sys_cell::kill_with_parent();
    let Some(plan) = CellPlan::parse(words) else {
        return failed(&mut status, CellFailure::Arguments);
    };
    if let Err(failure) = enter(&plan) {
        return failed(&mut status, failure);
    }
    let spawned = this_program().and_then(|me| {
        ultar_sys::set_inherited(&status, true)
            .map_err(|e| std::io::Error::from_raw_os_error(e.0))?;
        let child = Command::new(me).arg(CELL_INIT_ARG).args(words).spawn();
        let _ = ultar_sys::set_inherited(&status, false);
        child
    });
    let Ok(child) = spawned else {
        return failed(&mut status, CellFailure::Spawn);
    };
    report(&mut status, StatusLine::Cell(child.id()));
    let whole_cell = plan.namespaces.pid;
    let ended = relay_until_ended(&child, |signal| match signal {
        SIGTERM => Relay::Send(StopSignal::Terminate),
        SIGUSR1 if whole_cell => Relay::Send(StopSignal::Kill),
        SIGUSR1 => Relay::Send(StopSignal::KillRequest),
        _ => Relay::Reap,
    });
    let line = match ended {
        Ok(Ended::Code(code)) if code > SIGNALLED && code <= SIGNALLED + 64 => {
            StatusLine::Signal(code - SIGNALLED)
        }
        Ok(Ended::Code(code)) => StatusLine::Exit(code),
        Ok(Ended::Signal(signal)) => StatusLine::Signal(signal),
        Err(_) => StatusLine::Exit(-1),
    };
    report(&mut status, line);
    0
}

/// Ids, limits, rights and signals of stage 2 (after the view is built).
fn confine(plan: &CellPlan) -> Result<(), CellFailure> {
    ultar_sys::set_umask(0o077);
    ultar_sys::set_limit(Resource::Core, 0, 0).map_err(|_| CellFailure::Setup)?;
    // The limit counts every process and thread of the user. An app gets a user of its own
    // and with it this limit; a cell that stays the launcher's own user (a build under
    // test, started without the right to change users) would count that user's other
    // programs too and could not start – there the limit is not set.
    if let (Some(n), Some(_)) = (plan.limits.nproc, plan.identity) {
        ultar_sys::set_limit(Resource::Processes, n, n).map_err(|_| CellFailure::Setup)?;
    }
    if let Some((uid, gid)) = plan.identity {
        let identity = Identity {
            uid,
            gid,
            groups: plan.groups.clone(),
        };
        ultar_sys::switch_identity(&identity).map_err(|_| CellFailure::Identity)?;
    }
    // Linux forgets it when the ids change: set again after the switch
    let _ = sys_cell::kill_with_parent();
    // A kernel without the flag honours no set-id bits either: detected by
    // its answer; any other refusal stops the start.
    match ultar_sys::set_no_new_privileges() {
        Ok(()) | Err(Errno::ENOSYS | Errno::EINVAL) => {}
        Err(_) => return Err(CellFailure::Rights),
    }
    if let Some(mask) = plan.limits.drop_rights {
        ultar_sys::ultar_drop_rights(mask).map_err(|_| CellFailure::Rights)?;
        let left = ultar_sys::ultar_rights().map_err(|_| CellFailure::Rights)?;
        if left & mask != 0 {
            return Err(CellFailure::Rights);
        }
    }
    Ok(())
}

/// Stage 2: view, ids, rights, then the app. Returns the app's end as an
/// exit code (`128 + signal` for a signal).
pub fn stage_two(words: &[String]) -> i32 {
    let Some(mut status) = take_status() else {
        return CellFailure::Arguments as i32;
    };
    let _ = sys_cell::kill_with_parent();
    let Some(plan) = CellPlan::parse(words) else {
        return failed(&mut status, CellFailure::Arguments);
    };
    let workdir = if plan.namespaces.mount {
        if let Err(failure) = view::build(&plan) {
            return failed(&mut status, failure);
        }
        Path::new(view::DATA).to_owned()
    } else {
        plan.data.clone()
    };
    if std::env::set_current_dir(&workdir).is_err() {
        return failed(&mut status, CellFailure::Setup);
    }
    if let Err(failure) = confine(&plan) {
        return failed(&mut status, failure);
    }
    match supervise(status, &plan) {
        Ok(Ended::Code(code)) => code,
        Ok(Ended::Signal(signal)) => SIGNALLED + signal,
        Err(failure) => failure as i32,
    }
}

/// Stage 2 stays the cell's pid 1 (a pid 1 gets no default signal
/// actions, the app must): starts the app as pid 2 with no blocked signal,
/// passes `SIGTERM` on (`SIGUSR1`: `SIGKILL` to the app), reaps every
/// child, and ends with the app – the kernel then ends the rest of the
/// cell. After `READY` the status pipe is closed (review L-9): pid 1 runs as
/// the app's user, and the app must not be able to reach it; stage 1
/// reports the end.
fn supervise(mut status: File, plan: &CellPlan) -> Result<Ended, CellFailure> {
    // the app starts with an empty mask; this process blocks only now
    ultar_sys::set_signal_mask(0).map_err(|_| CellFailure::Setup)?;
    let started = Command::new(&plan.program.exec)
        .args(&plan.program.args)
        .env_remove(STATUS_ENV)
        .spawn();
    let app = match started {
        Ok(app) => app,
        Err(_) => {
            report(&mut status, StatusLine::Fail(CellFailure::Exec));
            return Err(CellFailure::Exec);
        }
    };
    if ultar_sys::block_signals(MASK).is_err() {
        report(&mut status, StatusLine::Fail(CellFailure::Setup));
        let _ = sys_cell::send_signal(app.id(), StopSignal::Kill);
        return Err(CellFailure::Setup);
    }
    report(&mut status, StatusLine::Ready);
    drop(status);
    relay_until_ended(&app, |signal| match signal {
        SIGTERM => Relay::Send(StopSignal::Terminate),
        SIGUSR1 => Relay::Send(StopSignal::Kill),
        _ => Relay::Reap,
    })
}

fn can_bind_a_node(scratch: &Path) -> bool {
    let node = scratch.join("node");
    sys_cell::make_mounts_private().is_ok()
        && sys_cell::mount_tmpfs(scratch, "size=65536,mode=0700").is_ok()
        && File::create(&node).is_ok()
        && sys_cell::bind(
            &Path::new(sys_cell::DEV).join(sys_cell::DEV_NODES[0]),
            &node,
            BindMode::Device,
        )
        .is_ok()
}

/// The platform probe (`--probe DIR`): tries the namespaces and a bind of
/// one device node onto a file in `DIR` (inside its own mount namespace,
/// nothing reaches the host). Exit code: [`PROBE_BASE`] + bits.
pub fn probe(scratch: &Path) -> i32 {
    let mut bits = 0;
    let cell = Namespaces {
        mount: true,
        pid: true,
        ..Namespaces::default()
    };
    if sys_cell::unshare(cell).is_ok() {
        bits |= PROBE_CELL;
        if can_bind_a_node(scratch) {
            bits |= PROBE_FILE_BIND;
        }
    }
    if sys_cell::unshare(Namespaces {
        net: true,
        ..Namespaces::default()
    })
    .is_ok()
    {
        bits |= PROBE_NET;
        // the probe is a throwaway process: setting the one-way barrier here costs nothing
        if matches!(set_net_guard(), Ok(true)) {
            bits |= PROBE_NET_GUARD;
        }
    }
    if sys_cell::unshare(Namespaces {
        ipc: true,
        ..Namespaces::default()
    })
    .is_ok()
    {
        bits |= PROBE_IPC;
    }
    PROBE_BASE + bits
}
