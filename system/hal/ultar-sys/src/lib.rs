//! # ultar-sys – the system calls `std` does not offer
//!
//! **Area:** system HAL (area 3, plan §3.0). The service runtime
//! (`ultar-service`) needs a handful of kernel services that `std` has no
//! API for. This crate offers them as a small **safe** API, without a C
//! library, so that no crate above the HAL contains `unsafe`.
//!
//! ## Which route each call takes
//!
//! `rustix` (already in the workspace lock; its `linux_raw` backend issues
//! system calls itself) is used wherever it has a safe function:
//!
//! | Call | Route |
//! |---|---|
//! | resource limits ([`limit`], [`set_limit`]) | `rustix::process::{getrlimit, setrlimit}` |
//! | file creation mask ([`set_umask`]) | `rustix::process::umask` |
//! | user and group ids ([`uid`], [`euid`], [`gid`]) | `rustix::process::get*id` |
//! | wait for two descriptors ([`wait_readable`]) | `rustix::event::poll` |
//! | no new privileges ([`set_no_new_privileges`]) | `rustix::thread::set_no_new_privs` |
//!
//! What `rustix` 1.1 cannot do safely is in the module `raw` – the only
//! file with `unsafe`, every block with a `SAFETY` comment:
//!
//! | Call | Why not `rustix` |
//! |---|---|
//! | [`block_signals`], [`signal_fd`] | no `signalfd` at all (listed under `not_implemented`); `sigprocmask` only in the unsupported `runtime` feature |
//! | [`peer_credentials`] | `rustix::net::sockopt::socket_peercred` **panics** when the kernel returns fewer than 12 bytes and builds a non-zero process id type from whatever it got. The Ultar kernel answers unknown socket options with 4 zero bytes; this crate has to recognise that and fail closed |
//! | [`ultar_rights`], [`ultar_drop_rights`] | `prctl` options of the Ultar kernel, unknown to `rustix` |
//! | [`is_open`], [`adopt_fd`], [`close_inherited`] | work on descriptor *numbers* inherited from the parent; turning a number into an owned or borrowed descriptor is `unsafe` in `std` and `rustix` alike |
//!
//! x86_64 and aarch64, Linux system call ABI (which the Ultar kernel
//! implements for these calls).

#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

mod raw;
#[cfg(test)]
mod tests;

pub use raw::{
    adopt_fd, block_signals, close_inherited, is_open, peer_credentials, signal_fd,
    ultar_drop_rights, ultar_rights, Ucred,
};

use rustix::event::{poll, PollFd, PollFlags};
use rustix::fs::Mode;
use rustix::process::{getrlimit, setrlimit, Rlimit};
use std::fmt;
use std::os::fd::BorrowedFd;

/// Error number returned by the kernel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Errno(pub i32);

impl Errno {
    /// Operation not permitted.
    pub const EPERM: Errno = Errno(1);
    /// Bad file descriptor.
    pub const EBADF: Errno = Errno(9);
    /// Invalid argument.
    pub const EINVAL: Errno = Errno(22);
    /// The kernel does not offer the requested socket option.
    pub const ENOPROTOOPT: Errno = Errno(92);
}

impl fmt::Display for Errno {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "errno {}", self.0)
    }
}

impl std::error::Error for Errno {}

impl From<rustix::io::Errno> for Errno {
    fn from(errno: rustix::io::Errno) -> Self {
        Self(errno.raw_os_error())
    }
}

/// A resource limit the runtime can set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resource {
    /// `RLIMIT_CORE`: size of core dumps.
    Core,
    /// `RLIMIT_NPROC`: processes and threads of the user.
    Processes,
    /// `RLIMIT_NOFILE`: open file descriptors.
    OpenFiles,
    /// `RLIMIT_AS`: address space in bytes.
    AddressSpace,
}

impl Resource {
    fn kernel(self) -> rustix::process::Resource {
        match self {
            Self::Core => rustix::process::Resource::Core,
            Self::Processes => rustix::process::Resource::Nproc,
            Self::OpenFiles => rustix::process::Resource::Nofile,
            Self::AddressSpace => rustix::process::Resource::As,
        }
    }
}

/// Reads the soft and the hard value of a limit of this process
/// (`u64::MAX`: unlimited).
pub fn limit(resource: Resource) -> Result<(u64, u64), Errno> {
    let now = getrlimit(resource.kernel());
    Ok((
        now.current.unwrap_or(u64::MAX),
        now.maximum.unwrap_or(u64::MAX),
    ))
}

/// Sets the soft and the hard value of a limit of this process
/// (`u64::MAX`: unlimited).
pub fn set_limit(resource: Resource, soft: u64, hard: u64) -> Result<(), Errno> {
    let value = |limit: u64| (limit != u64::MAX).then_some(limit);
    let new = Rlimit {
        current: value(soft),
        maximum: value(hard),
    };
    setrlimit(resource.kernel(), new).map_err(Errno::from)
}

/// Sets the file mode creation mask and returns the previous one.
pub fn set_umask(mask: u32) -> u32 {
    rustix::process::umask(Mode::from_bits_truncate(mask & 0o777)).bits() & 0o777
}

/// Real user id of this process.
pub fn uid() -> u32 {
    rustix::process::getuid().as_raw()
}

/// Effective user id of this process.
pub fn euid() -> u32 {
    rustix::process::geteuid().as_raw()
}

/// Real group id of this process.
pub fn gid() -> u32 {
    rustix::process::getgid().as_raw()
}

/// Forbids this process and its children to gain privileges through
/// `execve` (`PR_SET_NO_NEW_PRIVS`).
pub fn set_no_new_privileges() -> Result<(), Errno> {
    rustix::thread::set_no_new_privs(true).map_err(Errno::from)
}

/// Blocks until at least one of two descriptors is readable (or broken:
/// closed peer, error) and says which. Interrupted waits are repeated.
pub fn wait_readable(first: BorrowedFd<'_>, second: BorrowedFd<'_>) -> Result<(bool, bool), Errno> {
    let ready = PollFlags::IN | PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL;
    let mut fds = [
        PollFd::new(&first, PollFlags::IN),
        PollFd::new(&second, PollFlags::IN),
    ];
    loop {
        match poll(&mut fds, None) {
            Ok(_) => break,
            Err(rustix::io::Errno::INTR) => {}
            Err(errno) => return Err(errno.into()),
        }
    }
    Ok((
        fds[0].revents().intersects(ready),
        fds[1].revents().intersects(ready),
    ))
}
