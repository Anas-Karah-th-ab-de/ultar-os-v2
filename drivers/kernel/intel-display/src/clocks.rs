//! CDCLK operating points of display 14 and the PM-demand requests to the power controller
//! (voltage and clocks before a CDCLK change, the display's activity after a mode set).
//!
//! Values (38.4 MHz reference, PLL ratio, squash waveform, voltage levels) are Intel's published
//! data as used by Linux' `intel_cdclk.c` and `intel_pmdemand.c` (MIT).

use crate::cdclk::{CDCLK_CTL, DE_PLL_ENABLE, DE_PLL_LOCK, DE_PLL_ON};
use crate::error::{Error, Result};
use crate::hw::Hw;

/// `CDCLK_SQUASH_CTL`.
pub const CDCLK_SQUASH_CTL: u32 = 0x46008;
/// `XELPDP_INITIATE_PMDEMAND_REQUEST(0)`.
pub const PMDEMAND0: u32 = 0x45230;
/// `XELPDP_INITIATE_PMDEMAND_REQUEST(1)` (bit 31: request, cleared by the power controller).
pub const PMDEMAND1: u32 = 0x45234;

/// One CDCLK operating point of display 14 (38.4 MHz reference).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CdclkPoint {
    /// CDCLK in kHz.
    pub khz: u32,
    /// DE PLL ratio (VCO = ratio × 38.4 MHz).
    pub ratio: u32,
    /// Squash waveform (0 = no squashing).
    pub waveform: u16,
}

/// The operating points of display 14 (Meteor Lake / Arrow Lake).
pub const MTL_CDCLK: [CdclkPoint; 6] = [
    CdclkPoint {
        khz: 172_800,
        ratio: 16,
        waveform: 0xad5a,
    },
    CdclkPoint {
        khz: 192_000,
        ratio: 16,
        waveform: 0xb6b6,
    },
    CdclkPoint {
        khz: 307_200,
        ratio: 16,
        waveform: 0,
    },
    CdclkPoint {
        khz: 480_000,
        ratio: 25,
        waveform: 0,
    },
    CdclkPoint {
        khz: 556_800,
        ratio: 29,
        waveform: 0,
    },
    CdclkPoint {
        khz: 652_800,
        ratio: 34,
        waveform: 0,
    },
];

/// The lowest operating point of at least `need_khz` (at least 192 MHz, what the firmware uses).
pub fn choose(need_khz: u32) -> Result<CdclkPoint> {
    MTL_CDCLK
        .iter()
        .copied()
        .find(|p| p.khz >= need_khz.max(192_000))
        .ok_or(Error::CdclkTooLow {
            have: 652_800,
            need: need_khz,
        })
}

/// Voltage index the power controller needs for a CDCLK (Raptor Lake-U/Meteor Lake levels).
pub fn voltage_index(khz: u32) -> u32 {
    [312_000, 480_000, 556_800, 652_800]
        .iter()
        .position(|m| khz <= *m)
        .unwrap_or(3) as u32
}

/// `CDCLK_CTL` of an operating point: CD2X divider 1, no pipe, decimal frequency.
pub fn cdclk_ctl(p: &CdclkPoint) -> u32 {
    (7 << 19) | ((p.khz - 1000 + 250) / 500)
}

/// `CDCLK_SQUASH_CTL` of an operating point.
pub fn squash_ctl(p: &CdclkPoint) -> u32 {
    if p.waveform == 0 {
        0
    } else {
        (1 << 31) | (0xf << 24) | p.waveform as u32
    }
}

/// Asks the power controller for the voltage and clocks of `p` (and `dbufs` DBUF slices).
fn pmdemand<H: Hw>(hw: &mut H, p: &CdclkPoint, dbufs: u32) -> Result<()> {
    hw.wait_clear(PMDEMAND1, 1 << 31, 10_000)?;
    let v0 = hw.read(PMDEMAND0);
    let v0 = (v0 & !(0x7 << 12) & !(0x3 << 4)) | voltage_index(p.khz) << 12 | dbufs.min(3) << 4;
    // keep the request bit clear while the parameters change
    hw.write(PMDEMAND0, v0);
    let v1 = hw.read(PMDEMAND1) & !(0x7ff << 20);
    hw.write(PMDEMAND1, v1 | (p.khz / 1000) << 20 | 1 << 31);
    hw.wait_clear(PMDEMAND1, 1 << 31, 10_000).map(|_| ())
}

/// Sets CDCLK to `p` with the DE PLL off or about to be re-locked (no pipe may run).
pub fn set_cdclk<H: Hw>(hw: &mut H, p: &CdclkPoint) -> Result<()> {
    pmdemand(hw, p, 1)?;
    let pll = hw.read(DE_PLL_ENABLE);
    if pll & DE_PLL_ON != 0 && pll & 0xff != p.ratio {
        hw.rmw(DE_PLL_ENABLE, DE_PLL_ON, 0);
        hw.wait_clear(DE_PLL_ENABLE, DE_PLL_LOCK, 1000)?;
    }
    if hw.read(DE_PLL_ENABLE) & DE_PLL_LOCK == 0 {
        hw.write(DE_PLL_ENABLE, p.ratio);
        hw.write(DE_PLL_ENABLE, p.ratio | DE_PLL_ON);
        hw.wait_set(DE_PLL_ENABLE, DE_PLL_LOCK, 1000)?;
    }
    hw.write(CDCLK_SQUASH_CTL, squash_ctl(p));
    hw.write(CDCLK_CTL, cdclk_ctl(p));
    Ok(())
}

/// Tells the power controller what the display uses: `pipes`, `phys`, `plls` and the fastest
/// port clock (MHz). Without it, after a cold start it plans for an idle display (seen as a FIFO
/// underrun on the hardware).
pub fn pmdemand_activity<H: Hw>(
    hw: &mut H,
    pipes: u32,
    phys: u32,
    plls: u32,
    ddiclk_mhz: u32,
) -> Result<()> {
    hw.wait_clear(PMDEMAND1, 1 << 31, 10_000)?;
    let v0 = hw.read(PMDEMAND0) & !(0x3 << 6) & !0x7;
    hw.write(PMDEMAND0, v0 | pipes.min(3) << 6 | phys.min(7));
    let v1 = hw.read(PMDEMAND1) & !(0x7ff << 8) & !0x7;
    hw.write(
        PMDEMAND1,
        v1 | (ddiclk_mhz & 0x7ff) << 8 | plls.min(7) | 1 << 31,
    );
    hw.wait_clear(PMDEMAND1, 1 << 31, 10_000).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operating_points_match_linux_values() {
        // CDCLK_CTL under Linux: 0x0038017e at 192 MHz, 0x00380158 at 172.8 MHz
        assert_eq!(cdclk_ctl(&MTL_CDCLK[1]), 0x0038_017e);
        assert_eq!(cdclk_ctl(&MTL_CDCLK[0]), 0x0038_0158);
        assert_eq!(squash_ctl(&MTL_CDCLK[1]), 0x8f00_b6b6);
        assert_eq!(squash_ctl(&MTL_CDCLK[2]), 0);
        assert_eq!(choose(100).unwrap().khz, 192_000);
        assert_eq!(
            choose(326_700).unwrap(),
            CdclkPoint {
                khz: 480_000,
                ratio: 25,
                waveform: 0
            }
        );
        assert!(choose(700_000).is_err());
        assert_eq!(
            (
                voltage_index(192_000),
                voltage_index(480_000),
                voltage_index(556_800),
                voltage_index(652_800)
            ),
            (0, 1, 2, 3)
        );
        // popcount of the waveform scales VCO/2: 307.2 MHz x 10/16 = 192 MHz
        assert_eq!(307_200 * MTL_CDCLK[1].waveform.count_ones() / 16, 192_000);
        assert_eq!(307_200 * MTL_CDCLK[0].waveform.count_ones() / 16, 172_800);
    }
}
