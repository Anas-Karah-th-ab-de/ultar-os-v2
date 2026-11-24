//! What an app launcher needs to build a cell (PLAN.md §3.7): new mount,
//! pid, network and IPC namespaces, a private tree of mounts, the switch of
//! the root, and signals to the cell's first process.
//!
//! The calls are the Linux ones; the Ultar kernel implements the same
//! numbers (`docs/integration/kernel-isolation.md`) with these differences,
//! which callers detect by the answer, never by assuming:
//!
//! * no network, IPC or UTS namespaces: `unshare` answers `EINVAL`;
//! * bind mounts of directories only (a file answers `ENOTDIR`);
//! * one `mount` with `MS_BIND | MS_RDONLY` is already read-only (Linux
//!   needs the remount, which the Ultar kernel accepts as well);
//! * every namespace is private (propagation flags are accepted, no effect).
//!
//! The device nodes and kernel trees a cell may see are named here, so
//! that crates above the HAL name no kernel paths.

use crate::{raw, Errno};
use rustix::mount::{
    mount, mount_bind, mount_change, mount_remount, unmount, MountFlags, MountPropagationFlags,
    UnmountFlags,
};
use rustix::process::{kill_process, Pid, Signal};
use std::path::Path;

/// Where a cell gets its device nodes.
pub const DEV: &str = "/dev";
/// The device nodes a cell may use (all harmless: no hardware behind them).
pub const DEV_NODES: [&str; 5] = ["null", "zero", "full", "random", "urandom"];
/// Where a cell sees its own processes.
pub const PROC: &str = "/proc";

/// Which namespaces [`unshare`] creates.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Namespaces {
    /// Mount table (what the process sees of the file tree).
    pub mount: bool,
    /// Process ids (the next child becomes pid 1 of a new namespace).
    pub pid: bool,
    /// Network (only a loopback device, down).
    pub net: bool,
    /// System V IPC and POSIX message queues.
    pub ipc: bool,
}

impl Namespaces {
    /// The `CLONE_NEW*` flags of the set.
    pub const fn flags(self) -> usize {
        let mut flags = 0;
        if self.mount {
            flags |= 0x0002_0000;
        }
        if self.ipc {
            flags |= 0x0800_0000;
        }
        if self.pid {
            flags |= 0x2000_0000;
        }
        if self.net {
            flags |= 0x4000_0000;
        }
        flags
    }
}

/// Moves the calling process into new namespaces (pid: its next
/// children). Single-threaded callers only (a fresh helper process).
pub fn unshare(namespaces: Namespaces) -> Result<(), Errno> {
    raw::unshare_namespaces(namespaces.flags())
}

/// Stops mount events from travelling between the new mount namespace
/// and the one it was copied from (Linux; a no-op on the Ultar kernel).
pub fn make_mounts_private() -> Result<(), Errno> {
    mount_change(
        "/",
        MountPropagationFlags::PRIVATE | MountPropagationFlags::REC,
    )
    .map_err(Errno::from)
}

/// Mounts a fresh in-memory tree at `target` with `options`
/// (`size=`, `mode=`, `uid=`, `gid=`), without set-id programs, devices or
/// anything that can be executed from it (`nosuid,nodev,noexec`; the Ultar
/// kernel does not know `noexec` yet and ignores it).
pub fn mount_tmpfs(target: &Path, options: &str) -> Result<(), Errno> {
    let data = std::ffi::CString::new(options).map_err(|_| Errno::EINVAL)?;
    mount(
        "tmpfs",
        target,
        "tmpfs",
        MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC,
        Some(data.as_c_str()),
    )
    .map_err(Errno::from)
}

/// How [`bind`] exposes a source.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BindMode {
    /// Read-only, programs may run from it (system programs).
    ReadOnly,
    /// Writable, nothing may be executed from it (an app's data).
    WritableNoExec,
    /// A device node (read and write as its mode says).
    Device,
}

impl BindMode {
    fn flags(self) -> MountFlags {
        match self {
            Self::ReadOnly => MountFlags::RDONLY | MountFlags::NOSUID | MountFlags::NODEV,
            Self::WritableNoExec => MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC,
            Self::Device => MountFlags::NOSUID | MountFlags::NOEXEC,
        }
    }
}

/// Makes `source` (not its sub-mounts) appear at `target` with `mode`.
/// The bind is done in one call where the kernel honours the flags there
/// (Ultar kernel) and remounted with them (Linux needs the remount).
pub fn bind(source: &Path, target: &Path, mode: BindMode) -> Result<(), Errno> {
    let read_only = matches!(mode, BindMode::ReadOnly);
    if read_only {
        mount(
            source,
            target,
            "",
            MountFlags::BIND | MountFlags::RDONLY,
            None,
        )
        .map_err(Errno::from)?;
    } else {
        mount_bind(source, target).map_err(Errno::from)?;
    }
    mount_remount(target, MountFlags::BIND | mode.flags(), "").map_err(Errno::from)
}

/// Makes the mount at `target` read-only (the cell's own root after the
/// switch).
pub fn remount_read_only(target: &Path) -> Result<(), Errno> {
    mount_remount(
        target,
        MountFlags::RDONLY | MountFlags::NOSUID | MountFlags::NODEV,
        "",
    )
    .map_err(Errno::from)
}

/// Mounts the process tree at `target` (it shows the reader's pid
/// namespace).
pub fn mount_proc(target: &Path) -> Result<(), Errno> {
    mount(
        "proc",
        target,
        "proc",
        MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC,
        None,
    )
    .map_err(Errno::from)
}

/// Mounts the kernel's device tree at `target` (the Ultar kernel, whose
/// raw devices need the right `raw_device` and whose other nodes are
/// gated by their modes).
pub fn mount_devtmpfs(target: &Path) -> Result<(), Errno> {
    mount("dev", target, "devtmpfs", MountFlags::NOSUID, None).map_err(Errno::from)
}

/// Makes `new_root` (a mount point) the root and puts the old root at
/// `put_old` (a directory below `new_root`).
pub fn pivot_root(new_root: &Path, put_old: &Path) -> Result<(), Errno> {
    rustix::process::pivot_root(new_root, put_old).map_err(Errno::from)
}

/// Removes the mount at `target` and everything below it, even if busy.
pub fn detach(target: &Path) -> Result<(), Errno> {
    unmount(target, UnmountFlags::DETACH).map_err(Errno::from)
}

/// A signal a launcher sends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StopSignal {
    /// `SIGTERM`: please end.
    Terminate,
    /// `SIGKILL`: end now (for a namespace's pid 1: the whole namespace).
    Kill,
    /// `SIGUSR1`: a launcher's helper is asked to end its cell at once
    /// (the helper decides how; it is not a default action).
    KillRequest,
}

/// Sends `signal` to process `pid` (as this process sees the number).
pub fn send_signal(pid: u32, signal: StopSignal) -> Result<(), Errno> {
    let raw = i32::try_from(pid).map_err(|_| Errno::EINVAL)?;
    let pid = Pid::from_raw(raw).ok_or(Errno::EINVAL)?;
    let signal = match signal {
        StopSignal::Terminate => Signal::TERM,
        StopSignal::Kill => Signal::KILL,
        StopSignal::KillRequest => Signal::USR1,
    };
    kill_process(pid, signal).map_err(Errno::from)
}

/// Asks the kernel to end this process with `SIGKILL` when its parent
/// ends (`PR_SET_PDEATHSIG`; Linux clears it when the ids change, so call it
/// again after switching them). The Ultar kernel accepts the call without
/// doing it; callers there rely on the memory group.
pub fn kill_with_parent() -> Result<(), Errno> {
    rustix::process::set_parent_process_death_signal(Some(Signal::KILL)).map_err(Errno::from)
}

/// How a child ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ended {
    /// It returned this exit code.
    Code(i32),
    /// This signal ended it.
    Signal(i32),
}

/// Reaps one ended child of this process without waiting: `Ok(None)` when
/// none has ended (or there is no child at all).
pub fn reap_any() -> Result<Option<(u32, Ended)>, Errno> {
    match rustix::process::wait(rustix::process::WaitOptions::NOHANG) {
        Ok(Some((pid, status))) => {
            let ended = match (status.exit_status(), status.terminating_signal()) {
                (Some(code), _) => Ended::Code(code),
                (None, Some(signal)) => Ended::Signal(signal),
                (None, None) => return Ok(None),
            };
            Ok(Some((pid.as_raw_nonzero().get().unsigned_abs(), ended)))
        }
        Ok(None) | Err(rustix::io::Errno::CHILD) => Ok(None),
        Err(errno) => Err(errno.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_flags_are_the_linux_numbers() {
        assert_eq!(Namespaces::default().flags(), 0);
        let all = Namespaces {
            mount: true,
            pid: true,
            net: true,
            ipc: true,
        };
        assert_eq!(
            all.flags(),
            0x0002_0000 | 0x0800_0000 | 0x2000_0000 | 0x4000_0000
        );
        // nothing to do and foreign bits are refused before the kernel
        assert_eq!(unshare(Namespaces::default()), Err(Errno::EINVAL));
        assert_eq!(raw::unshare_namespaces(0x400), Err(Errno::EINVAL));
        assert_eq!(raw::unshare_namespaces(0x0002_0400), Err(Errno::EINVAL));
    }

    /// One test for everything that ends children of this process: tests
    /// run in parallel threads, and `reap_any` takes any child.
    #[test]
    #[allow(clippy::zombie_processes)] // reaped by `reap_any` (what is tested)
    fn signals_end_children_and_ended_children_are_reaped() {
        assert_eq!(send_signal(0, StopSignal::Terminate), Err(Errno::EINVAL));
        assert_eq!(send_signal(u32::MAX, StopSignal::Kill), Err(Errno::EINVAL));
        assert_eq!(kill_with_parent(), Ok(()));
        let sleeper = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        send_signal(sleeper.id(), StopSignal::Kill).unwrap();
        let exiting = std::process::Command::new("sh")
            .args(["-c", "exit 5"])
            .spawn()
            .unwrap();
        let mut wanted = vec![
            (sleeper.id(), Ended::Signal(9)),
            (exiting.id(), Ended::Code(5)),
        ];
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !wanted.is_empty() {
            if let Some(reaped) = reap_any().unwrap() {
                assert!(wanted.contains(&reaped), "{reaped:?}");
                wanted.retain(|w| *w != reaped);
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(reap_any(), Ok(None));
    }

    #[test]
    fn paths_are_absolute_and_nodes_plain_names() {
        assert!(DEV.starts_with('/') && PROC.starts_with('/'));
        assert!(DEV_NODES.iter().all(|n| !n.contains('/') && !n.is_empty()));
        let bad = std::ffi::CString::new("a\0b");
        assert!(bad.is_err());
        assert_eq!(
            mount_tmpfs(Path::new("/nonexistent-ultar"), "size=1\0m"),
            Err(Errno::EINVAL)
        );
    }
}
