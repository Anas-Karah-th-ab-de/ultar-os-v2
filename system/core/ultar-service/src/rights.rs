//! The rights of the Ultar kernel as a service declares them
//! (`kernel/ultar-kern/src/isolation/rights.rs`): a bit set per process
//! that is inherited and can only shrink.

/// `prctl` option of the Ultar kernel: read the rights of the process.
pub const PR_ULTAR_RIGHTS_GET: u32 = 0x554c_5200;
/// `prctl` option of the Ultar kernel: drop the rights in the mask.
pub const PR_ULTAR_RIGHTS_DROP: u32 = 0x554c_5201;
/// One right of the Ultar kernel (`kernel/ultar-kern/src/isolation/rights.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Right {
    /// Change the file tree view: mount, namespaces.
    Mount,
    /// Administer the network.
    NetAdmin,
    /// Load or unload kernel drivers.
    LoadDriver,
    /// Signal processes outside the own group.
    KillAny,
    /// Set the system clock.
    SetTime,
    /// Open raw devices.
    RawDevice,
    /// Restart or power off the machine.
    Reboot,
    /// Create and limit resource groups (`/sys/fs/cgroup`).
    ManageGroups,
    /// Change user and group ids (setuid, setgid, setgroups).
    SetId,
    /// Bypass file permission checks and change owners (chown).
    FileAdmin,
    /// Talk to a Bluetooth controller through its raw HCI node (Bluetooth service).
    Bluetooth,
    /// Create virtual input devices (Bluetooth service, remote control).
    InputInject,
    /// Play and record sound through the running streams `/dev/ultar/pcm` (call service).
    Sound,
}

impl Right {
    /// Every right the kernel knows.
    pub const ALL: [Right; 13] = [
        Right::Mount,
        Right::NetAdmin,
        Right::LoadDriver,
        Right::KillAny,
        Right::SetTime,
        Right::RawDevice,
        Right::Reboot,
        Right::ManageGroups,
        Right::SetId,
        Right::FileAdmin,
        Right::Bluetooth,
        Right::InputInject,
        Right::Sound,
    ];

    /// The kernel's bit for this right.
    pub const fn bit(self) -> u64 {
        match self {
            Self::Mount => 1 << 0,
            Self::NetAdmin => 1 << 1,
            Self::LoadDriver => 1 << 2,
            Self::KillAny => 1 << 3,
            Self::SetTime => 1 << 4,
            Self::RawDevice => 1 << 5,
            Self::Reboot => 1 << 6,
            Self::ManageGroups => 1 << 7,
            Self::SetId => 1 << 8,
            Self::FileAdmin => 1 << 9,
            Self::Bluetooth => 1 << 10,
            Self::InputInject => 1 << 11,
            Self::Sound => 1 << 12,
        }
    }
}

/// A set of kernel rights.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RightSet(u64);

impl RightSet {
    /// No right at all – what most services need.
    pub const NONE: RightSet = RightSet(0);
    /// Mask of all rights the kernel knows.
    pub const ALL_BITS: u64 = 0x1fff;

    /// The set of the given rights.
    pub fn of(rights: &[Right]) -> Self {
        Self(rights.iter().fold(0, |bits, right| bits | right.bit()))
    }

    /// Raw bit mask.
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// Whether `right` is in the set.
    pub const fn has(self, right: Right) -> bool {
        self.0 & right.bit() != 0
    }

    /// The mask to hand to `PR_ULTAR_RIGHTS_DROP` so that only this set
    /// remains. All 64 bits outside the set are dropped, so rights a later
    /// kernel adds are gone as well (default deny).
    pub const fn drop_mask(self) -> u64 {
        !self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_bits_match_the_kernel_and_the_drop_mask_is_default_deny() {
        let all = RightSet::of(&Right::ALL);
        assert_eq!(all.bits(), RightSet::ALL_BITS);
        for (index, right) in Right::ALL.iter().enumerate() {
            assert_eq!(right.bit(), 1 << index);
        }
        assert_eq!(RightSet::NONE.drop_mask(), u64::MAX);
        let driver = RightSet::of(&[Right::LoadDriver, Right::RawDevice]);
        assert!(driver.has(Right::LoadDriver) && !driver.has(Right::Reboot));
        assert_eq!(driver.drop_mask() & RightSet::ALL_BITS, 0x1fff & !0x24);
        // Bits of rights a later kernel may add are dropped too.
        assert_ne!(driver.drop_mask() & (1 << 40), 0);
        assert_eq!(RightSet::default(), RightSet::NONE);
    }
}
