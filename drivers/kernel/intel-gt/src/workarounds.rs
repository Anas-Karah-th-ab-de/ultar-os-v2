//! Hardware workarounds of Xe_LPG (graphics 12.70–12.74) that the kernel programs before
//! programs use the GT. Without them a compute shader can stop the engine for good (seen on the
//! Beelink: llama.cpp's MTP pass with flash attention never ended its dispatch on Ultar, while
//! Linux, which programs these, ran it).
//!
//! The list, register offsets and bits follow the MIT-licensed Linux `xe` driver (`xe_wa.c`,
//! `regs/xe_gt_regs.h`): the GT and engine entries whose rules cover graphics 12.70–12.74. The
//! context entries there only concern 3D rendering (not used yet). All registers here are
//! multicast (MCR); a plain write reaches every instance while the steering selects multicast.

use crate::Hw;

/// One workaround: bits set in a register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wa {
    /// Linux' name (the hardware issue numbers).
    pub name: &'static str,
    pub reg: u32,
    pub bits: u32,
    /// Masked register: the upper 16 bits select which lower bits a write changes.
    pub masked: bool,
}

/// `COMP_MOD_CTRL`.
const COMP_MOD_CTRL: u32 = 0xcf30;
/// `SQCNT1`.
const SQCNT1: u32 = 0x8718;
/// `ROW_CHICKEN3` (masked).
const ROW_CHICKEN3: u32 = 0xe49c;
/// `ROW_CHICKEN2` (masked).
const ROW_CHICKEN2: u32 = 0xe4f4;
/// `MTL_MCR_SELECTOR` and its multicast bit.
const MCR_SELECTOR: u32 = 0xfd4;
const MCR_MULTICAST: u32 = 1 << 31;

/// The workarounds for graphics 12.70–12.74 (GT, then first render/compute engine).
pub const XE_LPG: [Wa; 4] = [
    Wa { name: "14018575942", reg: COMP_MOD_CTRL, bits: 1 << 3, masked: false },
    Wa { name: "22016670082", reg: SQCNT1, bits: 1 << 23, masked: false },
    Wa { name: "14017856879", reg: ROW_CHICKEN3, bits: 1 << 9, masked: true },
    Wa { name: "14020495402", reg: ROW_CHICKEN2, bits: 1 << 1, masked: true },
];

/// Whether the workarounds apply to graphics version `ver` (×100, e.g. 1274).
pub fn applies(ver: u32) -> bool {
    (1270..=1274).contains(&ver)
}

/// Programs [`XE_LPG`] and returns, per workaround, whether its bits read back set.
pub fn apply(hw: &mut impl Hw) -> [bool; 4] {
    let sel = hw.read(MCR_SELECTOR);
    if sel & MCR_MULTICAST == 0 {
        hw.write(MCR_SELECTOR, MCR_MULTICAST);
    }
    let mut ok = [false; 4];
    for (i, wa) in XE_LPG.iter().enumerate() {
        if wa.masked {
            hw.write(wa.reg, wa.bits << 16 | wa.bits);
        } else {
            let v = hw.read(wa.reg);
            hw.write(wa.reg, v | wa.bits);
        }
        ok[i] = hw.read(wa.reg) & wa.bits == wa.bits;
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;

    #[test]
    fn sets_the_bits_masked_where_needed() {
        let mut hw = Sim::new();
        hw.set(SQCNT1, 0x10);
        hw.set(MCR_SELECTOR, MCR_MULTICAST);
        assert_eq!(apply(&mut hw), [true; 4]);
        assert_eq!(hw.get(COMP_MOD_CTRL), 1 << 3);
        assert_eq!(hw.get(SQCNT1), 0x10 | 1 << 23);
        assert_eq!(hw.writes_to(ROW_CHICKEN3), [1 << 25 | 1 << 9]);
        assert_eq!(hw.writes_to(ROW_CHICKEN2), [1 << 17 | 1 << 1]);
        assert!(hw.writes_to(MCR_SELECTOR).is_empty());
    }

    #[test]
    fn steering_set_to_multicast_first() {
        let mut hw = Sim::new();
        apply(&mut hw);
        assert_eq!(hw.writes_to(MCR_SELECTOR), [MCR_MULTICAST]);
    }

    #[test]
    fn only_xe_lpg() {
        assert!(applies(1274) && applies(1270));
        assert!(!applies(1260) && !applies(2004));
    }
}
