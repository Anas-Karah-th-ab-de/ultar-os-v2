//! Descriptors a process inherits from its parent: adopted or closed once,
//! at the very start, before the process opens anything itself.
//!
//! A descriptor *number* only names an inherited descriptor as long as the
//! process has not opened anything: afterwards the same number may belong
//! to a file the process opened. [`take_inherited`] therefore works only
//! once per process (later calls are refused) and the service runtime calls
//! it as the first thing of its start. Every declared descriptor is checked:
//! it must be open, above the standard streams, and a pipe or a regular
//! file – never a socket, a directory or a device.

use crate::raw::{adopt_number, close_number};
use crate::Errno;
use rustix::fs::FileType;
use std::collections::BTreeMap;
use std::fmt;
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by the first call of [`take_inherited`].
static TAKEN: AtomicBool = AtomicBool::new(false);

/// Why the inherited descriptors could not be taken over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InheritError {
    /// [`take_inherited`] was already called in this process.
    AlreadyTaken,
    /// A declared number is a standard stream (0–2) or negative.
    Reserved(i32),
    /// A declared number is not open: nothing was passed under it.
    NotOpen(i32),
    /// A declared descriptor is neither a pipe nor a regular file.
    WrongType(i32),
    /// The kernel refused to inspect a declared descriptor.
    Kernel(i32, Errno),
}

impl InheritError {
    /// The descriptor number the error is about, if any.
    pub const fn fd(&self) -> Option<i32> {
        match self {
            Self::AlreadyTaken => None,
            Self::Reserved(fd) | Self::NotOpen(fd) | Self::WrongType(fd) | Self::Kernel(fd, _) => {
                Some(*fd)
            }
        }
    }
}

impl fmt::Display for InheritError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyTaken => f.write_str("inherited descriptors were taken already"),
            Self::Reserved(fd) => write!(f, "descriptor {fd} is reserved"),
            Self::NotOpen(fd) => write!(f, "descriptor {fd} was not inherited"),
            Self::WrongType(fd) => write!(f, "descriptor {fd} is neither a pipe nor a file"),
            Self::Kernel(fd, errno) => write!(f, "descriptor {fd}: {errno}"),
        }
    }
}

impl std::error::Error for InheritError {}

/// The adopted descriptors, by number.
#[derive(Debug, Default)]
pub struct Inherited {
    owned: BTreeMap<i32, OwnedFd>,
    closed: usize,
}

impl Inherited {
    /// Hands out the owner of the declared descriptor `fd` (once).
    pub fn take(&mut self, fd: i32) -> Option<OwnedFd> {
        self.owned.remove(&fd)
    }

    /// Number of undeclared inherited descriptors that were closed.
    pub fn closed(&self) -> usize {
        self.closed
    }

    /// Numbers still held.
    pub fn numbers(&self) -> Vec<i32> {
        self.owned.keys().copied().collect()
    }
}

fn adopt_checked(fd: i32) -> Result<OwnedFd, InheritError> {
    if fd < 3 {
        return Err(InheritError::Reserved(fd));
    }
    let owned = adopt_number(fd).map_err(|_| InheritError::NotOpen(fd))?;
    let stat = rustix::fs::fstat(&owned).map_err(|errno| InheritError::Kernel(fd, errno.into()))?;
    match FileType::from_raw_mode(stat.st_mode) {
        FileType::Fifo | FileType::RegularFile => Ok(owned),
        // Dropping `owned` closes the descriptor: it is ours now.
        _ => Err(InheritError::WrongType(fd)),
    }
}

/// Takes over the declared inherited descriptors and, with `close_others`,
/// closes every other descriptor from 3 below `upper`. **Call first thing
/// in the process**; a second call is refused.
///
/// On an error the declared descriptors adopted so far are closed again;
/// the caller is expected to end the start.
pub fn take_inherited(
    declared: &[i32],
    close_others: bool,
    upper: i32,
) -> Result<Inherited, InheritError> {
    if TAKEN.swap(true, Ordering::SeqCst) {
        return Err(InheritError::AlreadyTaken);
    }
    let mut inherited = Inherited::default();
    for &fd in declared {
        if inherited.owned.contains_key(&fd) {
            continue;
        }
        inherited.owned.insert(fd, adopt_checked(fd)?);
    }
    if close_others {
        inherited.closed = (3..upper.max(3))
            .filter(|fd| !inherited.owned.contains_key(fd))
            .filter(|fd| close_number(*fd))
            .count();
    }
    Ok(inherited)
}
