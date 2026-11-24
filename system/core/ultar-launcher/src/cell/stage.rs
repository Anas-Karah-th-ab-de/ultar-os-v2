//! The two helper stages and the platform probe (see the module above).
//! These functions run in fresh, single-threaded helper processes.

use super::{view, CellFailure, CellPlan, StatusLine, CELL_INIT_ARG, STATUS_ENV};
use crate::platform::{PROBE_BASE, PROBE_CELL, PROBE_FILE_BIND, PROBE_IPC, PROBE_NET};
use std::fs::File;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::Command;
use ultar_sys::cell::{self as sys_cell, BindMode, Ended, Namespaces, StopSignal};
use ultar_sys::{Errno, Identity, Resource};

/// Highest descriptor number a stage closes at its start.
const CLOSE_UPPER: i32 = 1024;

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

/// Stage 1: group, namespaces, stage 2, then wait. Returns the exit code.
pub fn stage_one(words: &[String]) -> i32 {
    let Some(mut status) = take_status() else {
        return CellFailure::Arguments as i32;
    };
    let Some(plan) = CellPlan::parse(words) else {
        return failed(&mut status, CellFailure::Arguments);
    };
    if let Some(dir) = &plan.group_dir {
        if std::fs::write(dir.join("cgroup.procs"), std::process::id().to_string()).is_err() {
            return failed(&mut status, CellFailure::Group);
        }
    }
    if plan.namespaces != Namespaces::default() && sys_cell::unshare(plan.namespaces).is_err() {
        return failed(&mut status, CellFailure::Namespaces);
    }
    if plan.namespaces.mount && sys_cell::make_mounts_private().is_err() {
        return failed(&mut status, CellFailure::Mounts);
    }
    // this program; where the kernel cannot name it, the path it was started by
    let me = std::env::current_exe().or_else(|_| {
        std::env::args_os()
            .next()
            .map(std::path::PathBuf::from)
            .ok_or(std::io::ErrorKind::NotFound.into())
    });
    let spawned = me.and_then(|me| {
        ultar_sys::set_inherited(&status, true)
            .map_err(|e| std::io::Error::from_raw_os_error(e.0))?;
        let child = Command::new(me).arg(CELL_INIT_ARG).args(words).spawn();
        let _ = ultar_sys::set_inherited(&status, false);
        child
    });
    let Ok(mut child) = spawned else {
        return failed(&mut status, CellFailure::Spawn);
    };
    report(&mut status, StatusLine::Cell(child.id()));
    match child.wait() {
        Ok(end) => match (end.code(), end.signal()) {
            (Some(code), _) => report(&mut status, StatusLine::Exit(code)),
            (None, Some(signal)) => report(&mut status, StatusLine::Signal(signal)),
            (None, None) => report(&mut status, StatusLine::Exit(-1)),
        },
        Err(_) => report(&mut status, StatusLine::Exit(-1)),
    }
    0
}

/// Ids, limits, rights and signals of stage 2 (after the view is built).
fn confine(plan: &CellPlan) -> Result<(), CellFailure> {
    ultar_sys::set_umask(0o077);
    ultar_sys::set_limit(Resource::Core, 0, 0).map_err(|_| CellFailure::Setup)?;
    if let Some(n) = plan.nproc {
        ultar_sys::set_limit(Resource::Processes, n, n).map_err(|_| CellFailure::Setup)?;
    }
    if let Some((uid, gid)) = plan.identity {
        let identity = Identity {
            uid,
            gid,
            groups: Vec::new(),
        };
        ultar_sys::switch_identity(&identity).map_err(|_| CellFailure::Identity)?;
    }
    // A kernel without the flag honours no set-id bits either: detected by
    // its answer; any other refusal stops the start.
    match ultar_sys::set_no_new_privileges() {
        Ok(()) | Err(Errno::ENOSYS | Errno::EINVAL) => {}
        Err(_) => return Err(CellFailure::Rights),
    }
    if let Some(mask) = plan.drop_rights {
        ultar_sys::ultar_drop_rights(mask).map_err(|_| CellFailure::Rights)?;
        let left = ultar_sys::ultar_rights().map_err(|_| CellFailure::Rights)?;
        if left & mask != 0 {
            return Err(CellFailure::Rights);
        }
    }
    ultar_sys::set_signal_mask(0).map_err(|_| CellFailure::Setup)?;
    Ok(())
}

/// Stage 2: view, ids, rights, then the app. Returns only on failure.
pub fn stage_two(words: &[String]) -> i32 {
    let Some(mut status) = take_status() else {
        return CellFailure::Arguments as i32;
    };
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
    match supervise(&mut status, &plan) {
        Ok(line) => {
            report(&mut status, line);
            match line {
                StatusLine::Exit(code) => code,
                _ => 0,
            }
        }
        Err(failure) => failed(&mut status, failure),
    }
}

/// `SIGTERM`: forwarded to the app.
const SIGTERM: i32 = 15;
/// `SIGCHLD`: a child ended.
const SIGCHLD: i32 = 17;
/// Size of one `signalfd_siginfo` record.
const SIGINFO: usize = 128;

/// Stage 2 stays the cell's pid 1 (a pid 1 gets no default signal
/// actions, the app must): starts the app as pid 2, passes `SIGTERM` on,
/// reaps every child, and ends with the app – the kernel then ends the rest
/// of the cell. Returns how the app ended. `READY` is reported once the
/// signals are blocked: the launcher sends `SIGTERM` only after it read it
/// (a stop asked for before ends the cell with `SIGKILL`).
fn supervise(status: &mut File, plan: &CellPlan) -> Result<StatusLine, CellFailure> {
    use std::io::Read;
    // started before the signals are blocked: a child inherits the mask
    let app = Command::new(&plan.program.exec)
        .args(&plan.program.args)
        .env_remove(STATUS_ENV)
        .spawn()
        .map_err(|_| CellFailure::Exec)?;
    let pid = app.id();
    let mask = 1u64 << (SIGTERM - 1) | 1u64 << (SIGCHLD - 1);
    ultar_sys::block_signals(mask).map_err(|_| CellFailure::Setup)?;
    let mut signals = File::from(ultar_sys::signal_fd(mask).map_err(|_| CellFailure::Setup)?);
    // only now: the launcher signals this process once it read `READY`
    report(status, StatusLine::Ready);
    let mut info = [0u8; SIGINFO];
    loop {
        // also before the first wait: the app may have ended before the
        // signals were blocked (its SIGCHLD is then gone)
        while let Ok(Some((reaped, ended))) = sys_cell::reap_any() {
            if reaped == pid {
                return Ok(match ended {
                    Ended::Code(code) => StatusLine::Exit(code),
                    Ended::Signal(signal) => StatusLine::Signal(signal),
                });
            }
        }
        signals
            .read_exact(&mut info)
            .map_err(|_| CellFailure::Setup)?;
        let signal = i32::from_le_bytes([info[0], info[1], info[2], info[3]]);
        if signal == SIGTERM {
            let _ = sys_cell::send_signal(pid, StopSignal::Terminate);
        }
    }
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
