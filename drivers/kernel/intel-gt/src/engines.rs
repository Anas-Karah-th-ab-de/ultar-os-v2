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

/// Brings an engine at `base` into the state contexts can run in: no status page updates for
/// interrupts, context mode instead of the old ring mode, and its status page (one page mapped
/// at `status_page_ggtt`). Needs the engine's forcewake domain awake. The same three steps Linux
/// takes (`xe_hw_engine.c`: the engine's default registers and `xe_hw_engine_enable_ring`).
pub fn setup(hw: &mut impl Hw, base: u32, status_page_ggtt: u32) {
    hw.write(base + RING_HWSTAM, !0);
    hw.write(base + GFX_MODE, masked_enable(GFX_DISABLE_LEGACY_MODE));
    hw.write(base + RING_HWS_PGA, status_page_ggtt);
}

/// What an engine is doing, for reports when a job does not finish.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RingState {
    /// Ring head.
    pub head: u32,
    /// Ring tail.
    pub tail: u32,
    /// GGTT address of the ring.
    pub start: u32,
    /// Ring control.
    pub ctl: u32,
    /// Address of the command being executed.
    pub acthd: u32,
    /// Execution status.
    pub status: u32,
    /// The context image the engine runs.
    pub lrca: u32,
    /// Engine mode.
    pub mode: u32,
    /// Error status.
    pub esr: u32,
    /// Error identity.
    pub eir: u32,
    /// The command an error was raised at.
    pub ipehr: u32,
}

impl RingState {
    /// Reads the registers of the engine at `base`.
    pub fn read(hw: &mut impl Hw, base: u32) -> RingState {
        RingState {
            head: hw.read(base + RING_HEAD),
            tail: hw.read(base + RING_TAIL),
            start: hw.read(base + RING_START),
            ctl: hw.read(base + RING_CTL),
            acthd: hw.read(base + RING_ACTHD),
            status: hw.read(base + RING_EXECLIST_STATUS_LO),
            lrca: hw.read(base + RING_CURRENT_LRCA),
            mode: hw.read(base + GFX_MODE),
            esr: hw.read(base + RING_ESR),
            eir: hw.read(base + RING_EIR),
            ipehr: hw.read(base + RING_IPEHR),
        }
    }
}

impl core::fmt::Display for RingState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "head {:#x} tail {:#x} start {:#x} ctl {:#x} acthd {:#x} status {:#x} lrca {:#x} mode {:#x} esr {:#x} eir {:#x} ipehr {:#x}",
            self.head, self.tail, self.start, self.ctl, self.acthd, self.status, self.lrca,
            self.mode, self.esr, self.eir, self.ipehr
        )
    }
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

    #[test]
    fn setup_writes_the_three_registers() {
        let mut s = Sim::new();
        setup(&mut s, 0x22000, 0x0123_4000);
        assert_eq!(
            s.writes,
            [
                (0x22098, 0xffff_ffff),
                (0x2229c, 0x0008_0008),
                (0x22080, 0x0123_4000)
            ]
        );
    }

    #[test]
    fn ring_state_reads_and_prints() {
        let mut s = Sim::new();
        s.set(0x22034, 0x18);
        s.set(0x22030, 0x18);
        let st = RingState::read(&mut s, 0x22000);
        assert_eq!((st.head, st.tail), (0x18, 0x18));
        assert!(alloc::format!("{st}").starts_with("head 0x18 tail 0x18"));
    }
}
