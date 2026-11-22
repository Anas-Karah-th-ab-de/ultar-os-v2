//! Per-process rights: a bit set that is inherited and can only shrink.
//!
//! Programs started by the kernel hold every right (the behaviour before rights existed). A right is
//! checked at the system call it guards; there is no operation that adds a right.

/// Set of rights of one thread group.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rights(u64);

impl Rights {
    /// Change the file tree view: mount, umount, pivot_root, chroot, new namespaces, setns.
    pub const MOUNT: Rights = Rights(1 << 0);
    /// Administer the network: firewall and mesh configuration.
    pub const NET_ADMIN: Rights = Rights(1 << 1);
    /// Load or unload kernel drivers.
    pub const LOAD_DRIVER: Rights = Rights(1 << 2);
    /// Signal processes other than the own group and its descendants.
    pub const KILL_ANY: Rights = Rights(1 << 3);
    /// Set the system clock.
    pub const SET_TIME: Rights = Rights(1 << 4);
    /// Open raw devices (frame buffer, input devices).
    pub const RAW_DEVICE: Rights = Rights(1 << 5);
    /// Restart or power off the machine.
    pub const REBOOT: Rights = Rights(1 << 6);
    /// No right at all.
    pub const NONE: Rights = Rights(0);
    /// Every right: the set of programs started by the kernel.
    pub const ALL: Rights = Rights(0x7f);

    /// Raw bit mask (for `prctl`).
    pub fn bits(self) -> u64 {
        self.0
    }

    /// Whether every right in `other` is held.
    pub fn has(self, other: Rights) -> bool {
        self.0 & other.0 == other.0
    }

    /// Remove the rights named by the raw mask `mask`; unknown bits are ignored.
    pub fn drop_mask(&mut self, mask: u64) {
        self.0 &= !mask;
    }

    /// Right that answers to a Linux capability number, if this kernel models it.
    pub fn from_linux_cap(cap: u64) -> Option<Rights> {
        match cap {
            5 => Some(Rights::KILL_ANY),
            12 => Some(Rights::NET_ADMIN),
            16 => Some(Rights::LOAD_DRIVER),
            17 => Some(Rights::RAW_DEVICE),
            21 => Some(Rights::MOUNT),
            22 => Some(Rights::REBOOT),
            25 => Some(Rights::SET_TIME),
            _ => None,
        }
    }
}
