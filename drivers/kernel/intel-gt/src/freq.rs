//! The clock the GT computes with: its limits (fuses), what it runs at now, and a request for a
//! frequency. Without a request the GT stays near its lowest frequency and computes several
//! times slower than it can.
//!
//! Register offsets, fields and the request sequence follow `xe_guc_pc.c` of the MIT-licensed
//! Linux `xe` driver (`pc_set_cur_freq`, `mtl_get_rp0`, `mtl_get_cagf`). Xe_LPG (12.70–12.74).
//! All of them need the GT forcewake domain awake.

use crate::Hw;

/// `RPNSWREQ`: the frequency software asks for, as a ratio in bits 31:23.
pub const RPNSWREQ: u32 = 0xa008;
/// `RP_CONTROL`: bits 10:9 let the power unit take software requests.
pub const RP_CONTROL: u32 = 0xa024;
const RPSWCTL_MASK: u32 = 0x3 << 9;
const RPSWCTL_ENABLE: u32 = 0x2 << 9;
const RPSWCTL_DISABLE: u32 = 0;
/// `MTL_RP_STATE_CAP`: highest ratio in bits 8:0, lowest in bits 24:16.
pub const RP_STATE_CAP: u32 = 0x13_8000;
/// `MTL_MIRROR_TARGET_WP1`: the ratio the GT runs at now, bits 8:0.
pub const MIRROR_TARGET_WP1: u32 = 0xc60;
const RATIO_MASK: u32 = 0x1ff;

/// A frequency as the hardware counts it: units of 50/3 MHz.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ratio(pub u32);

impl Ratio {
    /// The frequency in MHz, rounded.
    pub fn mhz(self) -> u32 {
        (self.0 * 50 + 1) / 3
    }
}

/// The range the fuses allow: lowest and highest.
pub fn limits(hw: &mut impl Hw) -> (Ratio, Ratio) {
    let cap = hw.read(RP_STATE_CAP);
    (Ratio((cap >> 16) & RATIO_MASK), Ratio(cap & RATIO_MASK))
}

/// What the GT runs at now (0 while it sleeps).
pub fn current(hw: &mut impl Hw) -> Ratio {
    Ratio(hw.read(MIRROR_TARGET_WP1) & RATIO_MASK)
}

/// Asks the power unit for `ratio`: software requests are let through, the request is written,
/// and the gate is closed again so that nothing else rewrites it.
pub fn request(hw: &mut impl Hw, ratio: Ratio) {
    // only the gate bits change; the rest of the register stays as the power unit has it
    let control = hw.read(RP_CONTROL) & !RPSWCTL_MASK;
    hw.write(RP_CONTROL, control | RPSWCTL_ENABLE);
    hw.write(RPNSWREQ, (ratio.0 & RATIO_MASK) << 23);
    hw.write(RP_CONTROL, control | RPSWCTL_DISABLE);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;

    #[test]
    fn ratios_are_units_of_sixteen_and_two_thirds_megahertz() {
        assert_eq!(Ratio(3).mhz(), 50);
        assert_eq!(Ratio(135).mhz(), 2250);
        assert_eq!(Ratio(18).mhz(), 300);
    }

    #[test]
    fn limits_current_and_request_use_their_fields() {
        let mut hw = Sim::default();
        hw.set(RP_STATE_CAP, 18 << 16 | 135);
        hw.set(MIRROR_TARGET_WP1, 0xffff_fe00 | 42);
        assert_eq!(limits(&mut hw), (Ratio(18), Ratio(135)));
        assert_eq!(current(&mut hw), Ratio(42));
        hw.set(RP_CONTROL, 0x8000_0400 | 0x1);
        request(&mut hw, Ratio(135));
        assert_eq!(hw.get(RPNSWREQ), 135 << 23);
        assert_eq!(hw.get(RP_CONTROL), 0x8000_0001);
    }
}
