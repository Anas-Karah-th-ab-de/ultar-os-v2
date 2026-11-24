//! Which engines a GT has: the descriptor of its IP, reduced by the fuses. Bit positions follow
//! Linux' `enum xe_hw_engine_id` (`xe_hw_engine_types.h`, MIT), so masks compare with its logs.

use crate::ident::Ip;
use crate::regs::*;
use crate::{field, Hw};
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

/// Render engine.
pub const RCS0: u32 = 0;
/// First copy engine.
pub const BCS0: u32 = 1;
/// First video decode engine.
pub const VCS0: u32 = 10;
/// First video enhancement engine.
pub const VECS0: u32 = 18;
/// First compute engine.
pub const CCS0: u32 = 22;
/// The graphics security controller's engine.
pub const GSCCS0: u32 = 26;

const CLASSES: &[(&str, u32, u32)] = &[
    ("rcs", RCS0, 1),
    ("bcs", BCS0, 9),
    ("vcs", VCS0, 8),
    ("vecs", VECS0, 4),
    ("ccs", CCS0, 4),
    ("gsccs", GSCCS0, 1),
];

/// A set of engines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Engines(pub u32);

impl Engines {
    /// Whether engine `bit` is in the set.
    pub fn has(&self, bit: u32) -> bool {
        self.0 & (1 << bit) != 0
    }

    /// Number of engines.
    pub fn count(&self) -> u32 {
        self.0.count_ones()
    }

    /// Names as Linux prints them (`rcs0`, `ccs0`, `vcs2` …).
    pub fn names(&self) -> Vec<String> {
        let mut v = Vec::new();
        for (class, first, n) in CLASSES {
            for i in 0..*n {
                if self.has(first + i) {
                    let mut s = String::from(*class);
                    let _ = write!(s, "{i}");
                    v.push(s);
                }
            }
        }
        v
    }
}

/// Engines of the primary GT. Xe_LPG has one render, one copy and one compute engine; its copy
/// and compute fuses only matter for IPs with more than one of them.
pub fn primary(_ip: &Ip) -> Engines {
    Engines(1 << RCS0 | 1 << BCS0 | 1 << CCS0)
}

/// Engines of the media GT: up to 8 decode and 4 enhancement boxes as the fuses say, and the
/// security controller. Needs the media GT's `Gt` domain awake.
pub fn media(hw: &mut impl Hw) -> Engines {
    let fuse = hw.read(GT_VEBOX_VDBOX_DISABLE + MEDIA_GSI_OFFSET);
    if fuse == u32::MAX {
        return Engines(0);
    }
    // graphics 12.50 and later: a set bit means the box is present
    Engines(field(fuse, GT_VDBOX_MASK) << VCS0 | field(fuse, GT_VEBOX_MASK) << VECS0 | 1 << GSCCS0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;
    use alloc::vec;

    #[test]
    fn primary_gt_of_xe_lpg() {
        let e = primary(&Ip {
            verx100: 1274,
            name: "Xe_LPG+",
            revid: 4,
        });
        assert_eq!(e.names(), vec!["rcs0", "bcs0", "ccs0"]);
        assert_eq!(e.count(), 3);
    }

    #[test]
    fn media_boxes_from_the_fuses() {
        let mut s = Sim::new();
        // vcs0 + vcs2, vecs0: what Meteor Lake's media GT reports
        s.set(GT_VEBOX_VDBOX_DISABLE + MEDIA_GSI_OFFSET, 0x1_0005);
        let e = media(&mut s);
        assert_eq!(e.names(), vec!["vcs0", "vcs2", "vecs0", "gsccs0"]);
        s.set(GT_VEBOX_VDBOX_DISABLE + MEDIA_GSI_OFFSET, u32::MAX);
        assert_eq!(media(&mut s), Engines(0));
    }
}
