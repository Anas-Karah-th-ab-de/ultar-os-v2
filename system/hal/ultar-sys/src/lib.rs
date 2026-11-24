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
//! | wait for descriptors ([`wait_readable`], [`poll_readable`]) | `rustix::event::poll` |
//! | type of an inherited descriptor ([`take_inherited`]) | `rustix::fs::fstat` |
//! | no new privileges ([`set_no_new_privileges`]) | `rustix::thread::set_no_new_privs` |
//! | kernel device events ([`uevent::UeventSocket`]) | `rustix::net::{socket_with, bind, recv}` (`AF_NETLINK`, `NETLINK_KOBJECT_UEVENT`) |
//! | ids of a launched service ([`switch_identity`]), session ([`new_session`]), inherited descriptors ([`set_inherited`]) | `rustix::thread::set_thread_*`, `rustix::process::setsid`, `rustix::io::fcntl_setfd` |
//! | an app's cell ([`cell`]): mounts, root switch, signals | `rustix::mount::*`, `rustix::process::{pivot_root, kill_process}` |
//!
//! What `rustix` 1.1 cannot do safely is in the module `raw` – the only
//! file with `unsafe`, every block with a `SAFETY` comment:
//!
//! | Call | Why not `rustix` |
//! |---|---|
//! | [`block_signals`], [`set_signal_mask`], [`signal_fd`] | no `signalfd` at all (listed under `not_implemented`); `sigprocmask` only in the unsupported `runtime` feature |
//! | [`peer_credentials`] | `rustix::net::sockopt::socket_peercred` **panics** when the kernel returns fewer than 12 bytes and builds a non-zero process id type from whatever it got. The Ultar kernel answers unknown socket options with 4 zero bytes; this crate has to recognise that and fail closed |
//! | [`ultar_rights`], [`ultar_drop_rights`] | `prctl` options of the Ultar kernel, unknown to `rustix` |
//! | [`cell::unshare`] | `rustix` marks `unshare` unsafe because of `CLONE_FILES`; the raw call here passes namespace flags only |
//! | [`is_open`], [`take_inherited`] | work on descriptor *numbers* inherited from the parent; turning a number into an owned descriptor is `unsafe` in `std` and `rustix` alike. The safe API is one call per process ([`inherit`]) – no function hands out an owner for an arbitrary number |
//!
//! x86_64 and aarch64, Linux system call ABI (which the Ultar kernel
//! implements for these calls).

#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

mod raw;
#[cfg(test)]
mod tests;

pub mod cell;
mod identity;
pub mod inherit;
pub mod machine;
pub mod uevent;
mod wait;

pub use identity::{
    groups, is_inherited, new_session, set_controlling_terminal, set_inherited, switch_identity,
    Identity, IdentityError, MAX_GROUPS,
};
pub use inherit::{take_inherited, InheritError, Inherited};
pub use raw::{
    block_signals, is_open, peer_credentials, set_signal_mask, signal_fd, ultar_drop_rights,
    ultar_rights, Ucred,
};
pub use wait::{poll_readable, wait_readable};

use rustix::fs::Mode;
use rustix::process::{getrlimit, setrlimit, Rlimit};
use std::fmt;

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
    /// The kernel does not implement the call.
    pub const ENOSYS: Errno = Errno(38);
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

/// Effective group id of the process.
pub fn egid() -> u32 {
    rustix::process::getegid().as_raw()
}

/// Forbids this process and its children to gain privileges through
/// `execve` (`PR_SET_NO_NEW_PRIVS`).
pub fn set_no_new_privileges() -> Result<(), Errno> {
    rustix::thread::set_no_new_privs(true).map_err(Errno::from)
}
