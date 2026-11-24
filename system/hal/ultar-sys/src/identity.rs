//! What a launcher does to a fresh child before it executes a service:
//! a session of its own, other user and group ids, descriptors that are to
//! be inherited.
//!
//! The id calls change the ids of the **calling thread** (`rustix` routes
//! them to the raw system calls, which on Linux are per thread). A launcher
//! calls them while it is single-threaded – between `fork` and `execve`, or
//! in a freshly executed helper – where thread and process are the same.
//! Every change is read back: a kernel that answers 0 without doing it is
//! detected, not trusted.

use crate::Errno;
use rustix::io::FdFlags;
use rustix::process::{Gid, Uid};
use std::os::fd::AsFd;

/// The ids a service runs with.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Identity {
    /// User id (real, effective and saved).
    pub uid: u32,
    /// Group id (real, effective and saved).
    pub gid: u32,
    /// Supplementary groups (at most [`MAX_GROUPS`]).
    pub groups: Vec<u32>,
}

/// Most supplementary groups (the Ultar kernel keeps 16).
pub const MAX_GROUPS: usize = 32;

/// Why the ids could not be changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdentityError {
    /// More than [`MAX_GROUPS`] supplementary groups.
    TooManyGroups,
    /// The kernel refused a call.
    Kernel(Errno),
    /// The kernel answered success but the ids read back differ.
    NotApplied,
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyGroups => write!(f, "more than {MAX_GROUPS} groups"),
            Self::Kernel(errno) => write!(f, "{errno}"),
            Self::NotApplied => f.write_str("ids not applied"),
        }
    }
}

impl std::error::Error for IdentityError {}

/// Switches the calling thread to `identity`: groups first, then the group
/// id, the user id last (after it the right to change ids may be gone).
/// Only for a single-threaded launcher, see the module documentation.
pub fn switch_identity(identity: &Identity) -> Result<(), IdentityError> {
    if identity.groups.len() > MAX_GROUPS {
        return Err(IdentityError::TooManyGroups);
    }
    let groups: Vec<Gid> = identity
        .groups
        .iter()
        .map(|gid| Gid::from_raw_unchecked(*gid))
        .collect();
    let kernel = |error: rustix::io::Errno| IdentityError::Kernel(error.into());
    rustix::thread::set_thread_groups(&groups).map_err(kernel)?;
    let gid = Gid::from_raw_unchecked(identity.gid);
    rustix::thread::set_thread_res_gid(gid, gid, gid).map_err(kernel)?;
    let uid = Uid::from_raw_unchecked(identity.uid);
    rustix::thread::set_thread_res_uid(uid, uid, uid).map_err(kernel)?;
    if current_identity_matches(identity) {
        Ok(())
    } else {
        Err(IdentityError::NotApplied)
    }
}

/// Whether the real and effective ids and the groups are `identity`.
fn current_identity_matches(identity: &Identity) -> bool {
    let ids = rustix::process::getuid().as_raw() == identity.uid
        && rustix::process::geteuid().as_raw() == identity.uid
        && rustix::process::getgid().as_raw() == identity.gid
        && rustix::process::getegid().as_raw() == identity.gid;
    let mut groups: Vec<u32> = rustix::process::getgroups()
        .map(|list| list.iter().map(|gid| gid.as_raw()).collect())
        .unwrap_or_default();
    let mut wanted = identity.groups.clone();
    groups.sort_unstable();
    groups.dedup();
    wanted.sort_unstable();
    wanted.dedup();
    ids && groups == wanted
}

/// The current supplementary groups of the process.
pub fn groups() -> Result<Vec<u32>, Errno> {
    rustix::process::getgroups()
        .map(|list| list.iter().map(|gid| gid.as_raw()).collect())
        .map_err(Errno::from)
}

/// Starts a new session (and process group) led by the calling process,
/// so that a supervisor can signal the service and everything it starts.
pub fn new_session() -> Result<(), Errno> {
    rustix::process::setsid().map(|_| ()).map_err(Errno::from)
}

/// Makes the terminal `fd` the controlling terminal of the calling
/// session leader (a console service: Ctrl+C reaches it).
pub fn set_controlling_terminal(fd: impl AsFd) -> Result<(), Errno> {
    rustix::process::ioctl_tiocsctty(fd).map_err(Errno::from)
}

/// Marks `fd` as inherited by programs this process executes (`false`:
/// closed on `execve`, the default of every descriptor `std` opens).
pub fn set_inherited(fd: impl AsFd, inherited: bool) -> Result<(), Errno> {
    let flags = if inherited {
        FdFlags::empty()
    } else {
        FdFlags::CLOEXEC
    };
    rustix::io::fcntl_setfd(fd, flags).map_err(Errno::from)
}

/// Whether `fd` is inherited by programs this process executes.
pub fn is_inherited(fd: impl AsFd) -> Result<bool, Errno> {
    rustix::io::fcntl_getfd(fd)
        .map(|flags| !flags.contains(FdFlags::CLOEXEC))
        .map_err(Errno::from)
}
