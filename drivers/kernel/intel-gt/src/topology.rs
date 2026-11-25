//! Which parts of the GPU exist on this very chip: slices (DSS) for geometry and compute, execution
//! units per slice and L3 cache banks. The fuse registers say it; programs ask for it with the
//! device query "GT topology" and size their work by it (Mesa refuses a device without it).
//!
//! Follows Linux' MIT-licensed `xe_gt_topology.c` and `query_gt_topology` in `xe_query.c` closely
//! (register offsets, the bit arithmetic and the layout of the answer). Covers graphics 12.70 to
//! 12.74 (Xe_LPG); other IPs get no topology and the query is refused.

use crate::Hw;
use alloc::vec::Vec;

/// `XEHP_FUSE4`: bits 6:4 say which pairs of L3 banks exist.
pub const FUSE4: u32 = 0x9114;
/// `MIRROR_FUSE3`: bits 3:0 say which L3 nodes exist.
pub const FUSE3: u32 = 0x9118;
/// `XELP_EU_ENABLE`: one bit per pair of execution units in a slice.
pub const EU_ENABLE: u32 = 0x9134;
/// `XELP_GT_GEOMETRY_DSS_ENABLE`: one bit per slice that can do geometry.
pub const GEOMETRY_DSS_ENABLE: u32 = 0x913c;
/// `XEHP_GT_COMPUTE_DSS_ENABLE`: one bit per slice that can do compute.
pub const COMPUTE_DSS_ENABLE: u32 = 0x9144;

/// Answer record types (`DRM_XE_TOPO_*`).
const TOPO_DSS_GEOMETRY: u16 = 1;
const TOPO_DSS_COMPUTE: u16 = 2;
const TOPO_L3_BANK: u16 = 3;
const TOPO_EU_PER_DSS: u16 = 4;

/// The fuse registers as read from the device (GT forcewake domain awake).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Fuses {
    /// [`GEOMETRY_DSS_ENABLE`].
    pub geometry_dss: u32,
    /// [`COMPUTE_DSS_ENABLE`].
    pub compute_dss: u32,
    /// [`EU_ENABLE`].
    pub eu_enable: u32,
    /// [`FUSE3`].
    pub fuse3: u32,
    /// [`FUSE4`].
    pub fuse4: u32,
}

impl Fuses {
    /// Reads the five registers.
    pub fn read(hw: &mut impl Hw) -> Fuses {
        Fuses {
            geometry_dss: hw.read(GEOMETRY_DSS_ENABLE),
            compute_dss: hw.read(COMPUTE_DSS_ENABLE),
            eu_enable: hw.read(EU_ENABLE),
            fuse3: hw.read(FUSE3),
            fuse4: hw.read(FUSE4),
        }
    }
}

/// What the fuses mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Topology {
    /// Slices that can do geometry, one bit each.
    pub geometry_dss: u32,
    /// Slices that can do compute, one bit each.
    pub compute_dss: u32,
    /// Execution units of a slice, one bit each (8-wide units).
    pub eu_per_dss: u32,
    /// L3 cache banks, one bit each.
    pub l3_banks: u64,
}

/// Puts `pattern` (`bits` wide) at every position `mask` names (Linux:
/// `gen_l3_mask_from_pattern`).
fn replicate(pattern: u64, bits: u32, mask: u32) -> u64 {
    (0..32).filter(|i| mask & (1 << i) != 0).fold(0, |acc, i| acc | pattern.checked_shl(i * bits).unwrap_or(0))
}

impl Topology {
    /// The topology of a GT with graphics IP `verx100` (e.g. 1274); `None` for an IP this module
    /// does not know the fuse layout of.
    pub fn from_fuses(verx100: u32, f: &Fuses) -> Option<Topology> {
        if !(1270..=1274).contains(&verx100) {
            return None;
        }
        // one fuse bit stands for two execution units
        let eu_per_dss = (0..8).filter(|i| f.eu_enable & (1 << i) != 0).fold(0u32, |acc, i| acc | 0x3 << (2 * i));
        // one fuse bit stands for two banks of a node; the nodes sit four bits apart
        let per_node = replicate(0x3, 2, (f.fuse4 >> 4) & 0x7);
        let l3_banks = replicate(per_node, 4, f.fuse3 & 0xf);
        Some(Topology { geometry_dss: f.geometry_dss, compute_dss: f.compute_dss, eu_per_dss, l3_banks })
    }

    /// The records of GT `gt` for the device query, in Linux' order and sizes: geometry slices
    /// and compute slices (16 bytes each), L3 banks and execution units (8 bytes each).
    pub fn encode(&self, gt: u16) -> Vec<u8> {
        let mut out = Vec::new();
        let mut record = |kind: u16, mask: &[u8], len: usize| {
            out.extend_from_slice(&gt.to_le_bytes());
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&(len as u32).to_le_bytes());
            out.extend_from_slice(mask);
            out.resize(out.len() + len - mask.len(), 0);
        };
        record(TOPO_DSS_GEOMETRY, &self.geometry_dss.to_le_bytes(), 16);
        record(TOPO_DSS_COMPUTE, &self.compute_dss.to_le_bytes(), 16);
        record(TOPO_L3_BANK, &self.l3_banks.to_le_bytes(), 8);
        record(TOPO_EU_PER_DSS, &self.eu_per_dss.to_le_bytes(), 8);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replicate_follows_the_examples_of_the_linux_source() {
        assert_eq!(replicate(0b1111, 4, 0b0101), 0b0000_1111_0000_1111);
        assert_eq!(replicate(0xff, 8, 0b10), 0xff00);
        assert_eq!(replicate(0x3, 2, 0), 0);
    }

    #[test]
    fn fuses_become_masks() {
        let f = Fuses { geometry_dss: 0xf, compute_dss: 0xf, eu_enable: 0xff, fuse3: 0x1, fuse4: 0x30 };
        let t = Topology::from_fuses(1274, &f).unwrap();
        assert_eq!(t.eu_per_dss, 0xffff);
        assert_eq!(t.l3_banks, 0xf);
        assert_eq!((t.geometry_dss, t.compute_dss), (0xf, 0xf));
        // a half-populated slice: 3 fuse bits are 6 execution units
        assert_eq!(Topology::from_fuses(1270, &Fuses { eu_enable: 0b0111, ..f }).unwrap().eu_per_dss, 0x3f);
        // two L3 nodes, each with the first pair of banks
        assert_eq!(Topology::from_fuses(1271, &Fuses { fuse3: 0b11, fuse4: 0x10, ..f }).unwrap().l3_banks, 0x33);
    }

    #[test]
    fn unknown_ip_has_no_topology() {
        assert!(Topology::from_fuses(2004, &Fuses::default()).is_none());
        assert!(Topology::from_fuses(1260, &Fuses::default()).is_none());
    }

    #[test]
    fn answer_has_the_layout_of_linux() {
        let t = Topology { geometry_dss: 0x0f, compute_dss: 0x03, eu_per_dss: 0xffff, l3_banks: 0xf };
        let b = t.encode(0);
        assert_eq!(b.len(), 4 * 8 + 16 + 16 + 8 + 8);
        // gt 0, type 1, 16 bytes, mask
        assert_eq!(&b[..12], &[0, 0, 1, 0, 16, 0, 0, 0, 0x0f, 0, 0, 0]);
        // the last record: execution units
        assert_eq!(&b[b.len() - 16..b.len() - 6], &[0, 0, 4, 0, 8, 0, 0, 0, 0xff, 0xff]);
        assert_eq!(t.encode(1)[0], 1);
    }
}
