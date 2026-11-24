//! Display core initialisation of display 14 from a cold state (after power-on without firmware
//! display, a function level reset or resume): PCH/PICA reset handshake, power well 1 with its
//! fuse handshakes, CDCLK (DE PLL, squash waveform, `CDCLK_CTL`) with the PM-demand request to
//! the power controller, DBUF slice 1. Each step is skipped when the hardware is already there, so
//! calling it on a running display changes nothing.
//!
//! Step order and register fields follow Intel's "Display initialization sequence" as implemented
//! in Linux' `icl_display_core_init`, `intel_cdclk.c` and `intel_pmdemand.c` (MIT). The CDCLK
//! table values (38.4 MHz reference, PLL ratio, squash waveform) are Intel's published data.

use crate::cdclk::{CDCLK_CTL, DE_PLL_ENABLE, DE_PLL_LOCK};
use crate::clocks::{choose, set_cdclk};
use crate::error::{Error, Result};
use crate::hw::Hw;
use crate::power::{self, Well};

/// `HSW_NDE_RSTWRN_OPT`.
pub const NDE_RSTWRN_OPT: u32 = 0x46408;
/// Reset handshake with the PCH.
pub const RESET_PCH_HANDSHAKE: u32 = 1 << 4;
/// Reset handshake with PICA (display 14).
pub const RESET_PICA_HANDSHAKE: u32 = 1 << 6;
/// `SKL_FUSE_STATUS`.
pub const FUSE_STATUS: u32 = 0x42000;
/// `DC_STATE_EN`: hold the PHY latches of PG1 (to be cleared after PG1 on display 14).
pub const HOLD_PG1_LATCHES: u32 = (1 << 21) | (1 << 20);

/// Fuse distribution status bit of power gate `pg`.
pub const fn fuse_pg(pg: u32) -> u32 {
    1 << (27 - pg)
}

/// What `core_init` did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InitDone {
    /// Power well 1 was turned on.
    pub pw1: bool,
    /// CDCLK was set (kHz).
    pub cdclk: Option<u32>,
    /// DBUF slice 1 was turned on.
    pub dbuf: bool,
}

/// Brings the display core up far enough for a mode set of a pipe with `pixel_khz`;
/// `pipes_running` tells whether CDCLK may be changed (only with no pipe running).
pub fn core_init<H: Hw>(hw: &mut H, pixel_khz: u32, pipes_running: bool) -> Result<InitDone> {
    let mut done = InitDone::default();
    power::disable_dc_states(hw);
    hw.rmw(
        NDE_RSTWRN_OPT,
        0,
        RESET_PCH_HANDSHAKE | RESET_PICA_HANDSHAKE,
    );
    if !power::is_on(hw, Well::Pw1) {
        hw.wait_set(FUSE_STATUS, fuse_pg(0), 1000)?;
        power::enable(hw, Well::Pw1)?;
        hw.wait_set(FUSE_STATUS, fuse_pg(1), 1000)?;
        hw.rmw(power::DC_STATE_EN, HOLD_PG1_LATCHES, 0);
        done.pw1 = true;
    }
    let pll = hw.read(DE_PLL_ENABLE);
    let have = (pll & DE_PLL_LOCK != 0).then(|| crate::cdclk::from_ctl(hw.read(CDCLK_CTL)));
    let need = crate::cdclk::needed(&[pixel_khz], 14);
    match have {
        Some(h) if h >= need => {}
        Some(h) if pipes_running => return Err(Error::CdclkTooLow { have: h, need }),
        _ => {
            let p = choose(need)?;
            set_cdclk(hw, &p)?;
            done.cdclk = Some(p.khz);
        }
    }
    if hw.read(power::DBUF_CTL_S1) & power::DBUF_POWER_STATE == 0 {
        power::dbuf_on(hw, 0)?;
        done.dbuf = true;
    }
    if done.pw1 {
        // the arbiter registers live in power well 1: set them when it came up here
        bw_buddy(hw);
    }
    Ok(done)
}

/// `MTL_MEM_SS_INFO_GLOBAL`: memory type (3:0) and populated channels (7:4).
pub const MEM_SS_INFO: u32 = 0x45700;
/// `BW_BUDDY_CTL` of arbiter box 0 and 1 (stride 0x10).
pub const BW_BUDDY_CTL: u32 = 0x45130;
/// `BW_BUDDY_PAGE_MASK` of arbiter box 0 and 1.
pub const BW_BUDDY_PAGE_MASK: u32 = 0x45134;
/// `BW_BUDDY_CTL`: buddy logic off.
pub const BW_BUDDY_DISABLE: u32 = 1 << 31;

/// Page mask of the bandwidth buddy logic for memory type `ty` (MTL encoding) and `channels`;
/// `None`: unknown configuration (the logic is then turned off, as Linux does).
pub fn buddy_mask(ty: u32, channels: u32) -> Option<u32> {
    // MTL types: 0 DDR4, 1 DDR5, 2 LPDDR5, 3 LPDDR4
    match (channels, ty) {
        (1, 0) | (1, 1) => Some(0xf),
        (2, 3) | (2, 2) => Some(0x1c),
        (2, 0) => Some(0x1f),
        (2, 1) => Some(0x1e),
        (4, 3) | (4, 2) => Some(0x38),
        _ => None,
    }
}

/// Programs the arbiter's bandwidth buddy logic of both arbiter boxes for the installed memory.
pub fn bw_buddy<H: Hw>(hw: &mut H) {
    let info = hw.read(MEM_SS_INFO);
    let mask = buddy_mask(info & 0xf, (info >> 4) & 0xf);
    for abox in 0..2u32 {
        match mask {
            Some(m) => hw.write(BW_BUDDY_PAGE_MASK + 0x10 * abox, m),
            None => hw.write(BW_BUDDY_CTL + 0x10 * abox, BW_BUDDY_DISABLE),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdclk::DE_PLL_ON;
    use crate::sim::Sim;
    use alloc::boxed::Box;

    /// A display engine after a function level reset: nothing on, the hardware answers requests.
    fn cold() -> Sim {
        let mut s = Sim::new();
        s.set(FUSE_STATUS, fuse_pg(0) | fuse_pg(1));
        s.on_write(
            power::PWR_WELL_CTL2,
            Box::new(|regs, reg, v| {
                regs.insert(reg, v | (v & 0xaaaa_aaaa) >> 1);
            }),
        );
        s.on_write(
            DE_PLL_ENABLE,
            Box::new(|regs, reg, v| {
                regs.insert(
                    reg,
                    if v & DE_PLL_ON != 0 {
                        v | DE_PLL_LOCK
                    } else {
                        v & !DE_PLL_LOCK
                    },
                );
            }),
        );
        s.on_write(
            crate::clocks::PMDEMAND1,
            Box::new(|regs, reg, v| {
                regs.insert(reg, v & !(1 << 31));
            }),
        );
        s.on_write(
            power::DBUF_CTL[0],
            Box::new(|regs, reg, v| {
                regs.insert(
                    reg,
                    if v & power::DBUF_POWER_REQUEST != 0 {
                        v | power::DBUF_POWER_STATE
                    } else {
                        v
                    },
                );
            }),
        );
        s
    }

    #[test]
    fn cold_start_brings_everything_up() {
        let mut s = cold();
        let d = core_init(&mut s, 148_500, false).unwrap();
        assert_eq!(
            d,
            InitDone {
                pw1: true,
                cdclk: Some(192_000),
                dbuf: true
            }
        );
        assert_eq!(s.get(DE_PLL_ENABLE), 16 | DE_PLL_ON | DE_PLL_LOCK);
        assert_eq!(s.get(CDCLK_CTL), 0x0038_017e);
        assert_eq!(s.get(crate::clocks::CDCLK_SQUASH_CTL), 0x8f00_b6b6);
        assert!(power::is_on(&mut s, Well::Pw1));
        assert_eq!(
            s.get(NDE_RSTWRN_OPT),
            RESET_PCH_HANDSHAKE | RESET_PICA_HANDSHAKE
        );
        assert_eq!(s.get(crate::clocks::PMDEMAND1) >> 20 & 0x7ff, 192);
        assert_eq!(s.get(crate::clocks::PMDEMAND0) >> 4 & 3, 1);
        // a second call changes nothing
        let n = s.writes.len();
        let again = core_init(&mut s, 148_500, true).unwrap();
        assert_eq!(again, InitDone::default());
        assert_eq!(
            s.writes
                .iter()
                .skip(n)
                .filter(|w| w.0 != NDE_RSTWRN_OPT)
                .count(),
            0
        );
    }

    #[test]
    fn faster_clock_only_without_running_pipes() {
        let mut s = cold();
        core_init(&mut s, 148_500, false).unwrap();
        assert_eq!(
            core_init(&mut s, 594_000, true),
            Err(Error::CdclkTooLow {
                have: 192_000,
                need: 326_700
            })
        );
        let d = core_init(&mut s, 594_000, false).unwrap();
        assert_eq!(d.cdclk, Some(480_000));
        assert_eq!(s.get(DE_PLL_ENABLE) & 0xff, 25);
        assert_eq!(s.get(crate::clocks::CDCLK_SQUASH_CTL), 0);
        assert_eq!(s.get(crate::clocks::PMDEMAND0) >> 12 & 7, 1);
    }

    #[test]
    fn buddy_and_activity() {
        assert_eq!(buddy_mask(2, 2), Some(0x1c));
        assert_eq!(buddy_mask(1, 2), Some(0x1e));
        assert_eq!(buddy_mask(7, 2), None);
        let mut s = cold();
        s.set(MEM_SS_INFO, 0x24); // LPDDR4 x2? no: type 4 = DDR3 -> unknown
        bw_buddy(&mut s);
        assert_eq!(s.get(BW_BUDDY_CTL), BW_BUDDY_DISABLE);
        assert_eq!(s.get(BW_BUDDY_CTL + 0x10), BW_BUDDY_DISABLE);
        s.set(MEM_SS_INFO, 0x42); // LPDDR5, 4 channels
        bw_buddy(&mut s);
        assert_eq!(s.get(BW_BUDDY_PAGE_MASK), 0x38);
        crate::clocks::pmdemand_activity(&mut s, 1, 1, 1, 162).unwrap();
        assert_eq!(s.get(crate::clocks::PMDEMAND0) & 0xc7, 1 << 6 | 1);
        assert_eq!(s.get(crate::clocks::PMDEMAND1) >> 8 & 0x7ff, 162);
        assert_eq!(s.get(crate::clocks::PMDEMAND1) & 7, 1);
    }

    #[test]
    fn missing_fuses_and_silent_controller_fail() {
        let mut s = cold();
        s.set(FUSE_STATUS, 0);
        assert!(matches!(
            core_init(&mut s, 148_500, false),
            Err(Error::Timeout {
                reg: FUSE_STATUS,
                ..
            })
        ));
        let mut t = cold();
        t.on_write(
            crate::clocks::PMDEMAND1,
            Box::new(|regs, reg, v| {
                regs.insert(reg, v);
            }),
        );
        assert!(matches!(
            core_init(&mut t, 148_500, false),
            Err(Error::Timeout {
                reg: crate::clocks::PMDEMAND1,
                ..
            })
        ));
    }
}
