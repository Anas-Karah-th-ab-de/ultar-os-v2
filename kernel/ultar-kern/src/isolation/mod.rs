//! Process isolation of the Ultar kernel, stage 1 (plan §3.7): mount namespaces, pid namespaces and
//! the foundation of per-process rights.
//!
//! A *cell* is a process whose view of the machine is narrowed by these three building blocks:
//! what it sees of the file tree ([`mount`], [`path`], [`tmpfs`]), which processes it sees
//! ([`pid`], [`signals`]) and what it may do ([`rights`]). The system calls are in [`calls`].
//! Design and the escape argument: `DESIGN-isolation.md`.

pub mod calls;
pub mod mount;
pub mod path;
pub mod pid;
pub mod rights;
pub mod signals;
pub mod tmpfs;

use alloc::string::String;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};
use mount::MountNs;
use pid::PidNs;
use rights::Rights;

static NEXT_ROOT_ID: AtomicU64 = AtomicU64::new(1);

/// Identity of a file tree view: directory descriptors are only valid as a base inside the view
/// they were opened in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ViewTag {
    ns: u64,
    epoch: u64,
    root: u64,
}

/// A namespace a descriptor can refer to (`/proc/<pid>/ns/*`), for `setns`.
#[derive(Clone)]
pub enum NsRef {
    /// A mount namespace.
    Mnt(Arc<MountNs>),
    /// A pid namespace.
    Pid(Arc<PidNs>),
}

/// Isolation state of one thread group.
#[derive(Clone)]
pub struct Isolation {
    /// Mount namespace.
    pub mnt: Arc<MountNs>,
    /// Root directory of the group as a path in the mount namespace (`chroot`).
    pub root: String,
    /// Changes with every change of `root`.
    pub root_id: u64,
    /// Pid namespace new children are placed in (differs from the own one after `unshare(CLONE_NEWPID)`).
    pub pid_ns_children: Arc<PidNs>,
    /// Rights held.
    pub rights: Rights,
    /// User and group ids.
    pub cred: crate::cred::Cred,
}

impl Isolation {
    /// State of a program started by the kernel: the host tree, the initial pid namespace, every right.
    pub fn host() -> Isolation {
        Isolation {
            mnt: MountNs::initial(),
            root: String::from("/"),
            root_id: 0,
            pid_ns_children: PidNs::initial(),
            rights: Rights::ALL,
            cred: crate::cred::Cred::ROOT,
        }
    }

    /// Whether the view is narrower than the host tree (anything but the initial namespace with root `/`).
    pub fn confined(&self) -> bool {
        !self.mnt.is_initial() || self.root != "/"
    }

    /// Tag of the current view.
    pub fn view_tag(&self) -> ViewTag {
        ViewTag {
            ns: self.mnt.id(),
            epoch: self.mnt.epoch(),
            root: self.root_id,
        }
    }

    /// Set a new root directory (namespace path).
    pub fn set_root(&mut self, root: String) {
        self.root = root;
        self.root_id = NEXT_ROOT_ID.fetch_add(1, Ordering::Relaxed);
    }
}
