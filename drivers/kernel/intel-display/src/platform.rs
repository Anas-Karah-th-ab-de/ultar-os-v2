//! Which Intel graphics devices the driver takes, and what their display engine has.

use crate::error::{Error, Result};

/// Display generation and size of the display engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform {
    /// Short name for logs (`mtl`, `arl`, `cml`).
    pub name: &'static str,
    /// Display version (14 = Xe_LPD+, 9 = Skylake family).
    pub display_ver: u8,
    /// Number of pipes (= transcoders A..).
    pub pipes: u8,
    /// Universal planes per pipe (without the cursor).
    pub planes: u8,
}

impl Platform {
    /// Whether this driver can do a full mode set (enable a pipe from nothing) on it.
    pub fn can_modeset(&self) -> bool {
        self.display_ver == 14
    }

    /// Whether the Type-C ports and their PHYs are the Xe_LPD+ kind (PICA, C20 PHY).
    pub fn is_xelpdp(&self) -> bool {
        self.display_ver >= 14
    }
}

const MTL: Platform = Platform {
    name: "mtl",
    display_ver: 14,
    pipes: 4,
    planes: 5,
};
const ARL: Platform = Platform {
    name: "arl",
    display_ver: 14,
    pipes: 4,
    planes: 5,
};
const CML: Platform = Platform {
    name: "cml",
    display_ver: 9,
    pipes: 3,
    planes: 3,
};

/// PCI device ids (vendor 8086) of Meteor Lake and Arrow Lake integrated graphics.
pub const XELPDP_IDS: &[(u16, Platform)] = &[
    (0x7d40, MTL),
    (0x7d45, MTL),
    (0x7d55, MTL),
    (0x7d60, MTL),
    (0x7dd5, MTL),
    (0x7d51, ARL),
    (0x7dd1, ARL),
    (0x7d41, ARL),
    (0x7d67, ARL),
];

/// PCI device ids of Comet Lake GT2 graphics (display version 9).
pub const GEN9_IDS: &[(u16, Platform)] = &[
    (0x9bc2, CML),
    (0x9bc4, CML),
    (0x9bc5, CML),
    (0x9bc6, CML),
    (0x9bc8, CML),
    (0x9be6, CML),
    (0x9bf6, CML),
    (0x9b41, CML),
    (0x9bca, CML),
    (0x9bcc, CML),
];

/// The platform of Intel device `device`, if the driver supports it.
pub fn lookup(device: u16) -> Result<Platform> {
    XELPDP_IDS
        .iter()
        .chain(GEN9_IDS.iter())
        .find(|(d, _)| *d == device)
        .map(|(_, p)| *p)
        .ok_or(Error::UnknownDevice(device))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beelink_and_thinkpad_are_known() {
        let p = lookup(0x7d51).unwrap();
        assert_eq!((p.display_ver, p.pipes, p.planes), (14, 4, 5));
        assert!(p.can_modeset() && p.is_xelpdp());
        let t = lookup(0x9bc4).unwrap();
        assert_eq!(t.display_ver, 9);
        assert!(!t.can_modeset() && !t.is_xelpdp());
    }

    #[test]
    fn others_are_refused() {
        assert_eq!(lookup(0x1111), Err(Error::UnknownDevice(0x1111)));
        assert_eq!(lookup(0x56a0), Err(Error::UnknownDevice(0x56a0)));
    }

    #[test]
    fn ids_are_unique() {
        let all: alloc::vec::Vec<u16> = XELPDP_IDS
            .iter()
            .chain(GEN9_IDS.iter())
            .map(|x| x.0)
            .collect();
        for (i, a) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(a), "{a:#x} twice");
        }
    }
}
