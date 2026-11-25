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

/// Caching of the five "PAT indexes" page table entries can name on Xe_LPG, as Linux programs
/// them (`xe_pat.c`): 0 write-back, 1 write-through, 2 uncached, 3 write-back and the GPU sees
/// what is in the CPU's caches, 4 the same in both directions. The kernel maps memory of
/// programs with index 3, so nothing has to be flushed by hand.
pub const XE_LPG_PAT: [u32; 5] = [0, 1 << 2, 3 << 2, 2, 3];

/// Writes [`XE_LPG_PAT`]. Needs the GT forcewake domain awake.
pub fn program_pat(hw: &mut impl Hw) {
    for (i, value) in XE_LPG_PAT.iter().enumerate() {
        hw.write(PAT_INDEX0 + 4 * i as u32, *value);
    }
}

/// Frequency in Hz at which the engines' time stamp counters run, from the crystal the
/// firmware configured and the divider (`RPM_CONFIG0`); programs scale GPU times with it.
/// `None` for a crystal code that does not exist. As Linux' `xe_gt_clock.c`.
pub fn reference_clock(hw: &mut impl Hw) -> Option<u32> {
    let c0 = hw.read(RPM_CONFIG0);
    let crystal = match (c0 >> 3) & 7 {
        0 => 24_000_000u32,
        1 => 19_200_000,
        2 => 38_400_000,
        3 => 25_000_000,
        _ => return None,
    };
    Some(crystal >> (3 - ((c0 >> 1) & 3)))
}

/// Switches the compute engines on (off after reset). Needs the render forcewake domain awake.
pub fn enable_compute(hw: &mut impl Hw) {
    hw.write(RCU_MODE, masked_enable(RCU_MODE_CCS_ENABLE));
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
    fn pat_table_is_written_in_order() {
        let mut s = Sim::new();
        program_pat(&mut s);
        assert_eq!(
            s.writes,
            [
                (0x4800, 0),
                (0x4804, 4),
                (0x4808, 12),
                (0x480c, 2),
                (0x4810, 3)
            ]
        );
    }

    #[test]
    fn reference_clock_from_crystal_and_divider() {
        let mut s = Sim::new();
        // 38.4 MHz crystal, shift parameter 2: one tick every two crystal cycles
        s.set(0xd00, 2 << 3 | 2 << 1);
        assert_eq!(reference_clock(&mut s), Some(19_200_000));
        s.set(0xd00, 1 << 3 | 3 << 1);
        assert_eq!(reference_clock(&mut s), Some(19_200_000));
        s.set(0xd00, 5 << 3);
        assert_eq!(reference_clock(&mut s), None);
    }

    #[test]
    fn compute_is_switched_on_with_a_masked_write() {
        let mut s = Sim::new();
        enable_compute(&mut s);
        assert_eq!(s.writes, [(0x14800, 0x0001_0001)]);
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
