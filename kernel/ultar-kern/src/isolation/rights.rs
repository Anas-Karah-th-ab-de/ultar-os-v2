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
    /// Manage resource groups: create and remove groups, set or raise limits, move processes
    /// between groups (the group tree under `/sys/fs/cgroup`), raise the largest pipe size.
    pub const MANAGE_GROUPS: Rights = Rights(1 << 7);
    /// Change user and group ids freely (`setuid`, `setgid`, `setgroups` …); without it a process
    /// can only move between the ids it already has.
    pub const SET_ID: Rights = Rights(1 << 8);
    /// Act on files regardless of their owner and mode: read, write, search and remove anything,
    /// `chown`, `chmod` of files of others. (What "being root" means for files on Linux.)
    pub const FILE_ADMIN: Rights = Rights(1 << 9);
    /// Talk to a Bluetooth controller through its raw HCI node `/dev/ultar/hciN` (the Bluetooth
    /// service). Neither `RAW_DEVICE` nor `FILE_ADMIN` opens that node: only this right does.
    pub const BLUETOOTH: Rights = Rights(1 << 10);
    /// Create virtual input devices through `/dev/ultar/uinput` (the Bluetooth service for its
    /// keyboards and mice, remote control). Neither `RAW_DEVICE` nor `FILE_ADMIN` opens that node.
    pub const INPUT_INJECT: Rights = Rights(1 << 11);
    /// Play and record sound through the running streams `/dev/ultar/pcm` (the call service).
    /// Neither `RAW_DEVICE` nor `FILE_ADMIN` opens that node: only this right does.
    pub const SOUND: Rights = Rights(1 << 12);
    /// Take pictures from a camera through `/dev/ultar/kamera` (the call service). Neither
    /// `RAW_DEVICE` nor `FILE_ADMIN` opens that node: only this right does.
    pub const CAMERA: Rights = Rights(1 << 13);
    /// Change owner and group of entries inside the own cell – far narrower than `FILE_ADMIN`
    /// (`docs/integration/zell-chown.md`): only for a confined view, only on an entry that lies on
    /// one of the view's writable mounts whose source is not the whole host tree, never in the
    /// kernel's trees, and the set-id bits are always cleared. The install cell of foreign packages
    /// needs it (`dpkg`/`apt` hand files to `_apt` and other system users).
    pub const CELL_CHOWN: Rights = Rights(1 << 14);
    /// No right at all.
    pub const NONE: Rights = Rights(0);
    /// Every right: the set of programs started by the kernel.
    pub const ALL: Rights = Rights(0x7fff);

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
            // CAP_CHOWN, CAP_DAC_OVERRIDE, CAP_DAC_READ_SEARCH, CAP_FOWNER
            0..=3 => Some(Rights::FILE_ADMIN),
            // CAP_SETGID, CAP_SETUID
            6 | 7 => Some(Rights::SET_ID),
            5 => Some(Rights::KILL_ANY),
            12 => Some(Rights::NET_ADMIN),
            16 => Some(Rights::LOAD_DRIVER),
            17 => Some(Rights::RAW_DEVICE),
            21 => Some(Rights::MOUNT),
            22 => Some(Rights::REBOOT),
            24 => Some(Rights::MANAGE_GROUPS),
            25 => Some(Rights::SET_TIME),
            _ => None,
        }
    }
}
