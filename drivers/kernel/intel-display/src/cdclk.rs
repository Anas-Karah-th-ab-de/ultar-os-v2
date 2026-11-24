//! Core display clock (CDCLK). The driver reads what the firmware set and checks that it is fast
//! enough for a mode; changing it (DE PLL, squash/crawl and the PCODE handshake) is not
//! implemented, a mode that needs more is refused with [`Error::CdclkTooLow`].

use crate::error::{Error, Result};
use crate::hw::Hw;

/// `CDCLK_CTL`.
pub const CDCLK_CTL: u32 = 0x46000;
/// `BXT_DE_PLL_ENABLE` (display 10+: the CDCLK PLL).
pub const DE_PLL_ENABLE: u32 = 0x46070;
/// DE PLL enable.
pub const DE_PLL_ON: u32 = 1 << 31;
/// DE PLL lock.
pub const DE_PLL_LOCK: u32 = 1 << 30;

/// The CDCLK in kHz from `CDCLK_CTL`'s decimal field (frequency in MHz × 2 − 2 … in 0.5 MHz).
pub fn from_ctl(ctl: u32) -> u32 {
    (ctl & 0x7ff) * 500 + 1000
}

/// The CDCLK the hardware runs (`None` when its PLL is off or not locked).
pub fn read<H: Hw>(hw: &mut H, display_ver: u8) -> Option<u32> {
    if display_ver >= 10 {
        let pll = hw.read(DE_PLL_ENABLE);
        if pll & DE_PLL_ON == 0 || pll & DE_PLL_LOCK == 0 {
            return None;
        }
    }
    Some(from_ctl(hw.read(CDCLK_CTL)))
}

/// Lowest CDCLK for pipes with the given pixel rates (display 10+: two pixels per clock per pipe;
/// display 9: one), with the 10 % reserve Intel recommends for plane fetch.
pub fn needed(pixel_rates_khz: &[u32], display_ver: u8) -> u32 {
    let ppc = if display_ver >= 10 { 2 } else { 1 };
    let max = pixel_rates_khz.iter().copied().max().unwrap_or(0) as u64;
    (max.div_ceil(ppc) * 11 / 10) as u32
}

/// Checks that the current CDCLK carries the pixel rates.
pub fn check<H: Hw>(hw: &mut H, display_ver: u8, pixel_rates_khz: &[u32]) -> Result<u32> {
    let have = read(hw, display_ver).ok_or(Error::NotSupported(
        "CDCLK is off and enabling it is not implemented",
    ))?;
    let need = needed(pixel_rates_khz, display_ver);
    if have < need {
        return Err(Error::CdclkTooLow { have, need });
    }
    Ok(have)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;

    #[test]
    fn decimal_field() {
        // 192 MHz (the Beelink under Linux): (192 - 1) * 2 = 382
        assert_eq!(from_ctl(382), 192_000);
        assert_eq!(from_ctl(0x123_0000 | 1303), 652_500);
    }

    #[test]
    fn checks_rates() {
        let mut s = Sim::new();
        s.set(DE_PLL_ENABLE, DE_PLL_ON | DE_PLL_LOCK);
        s.set(CDCLK_CTL, 382);
        assert_eq!(check(&mut s, 14, &[148_500]), Ok(192_000));
        assert_eq!(check(&mut s, 14, &[148_500, 74_250]), Ok(192_000));
        assert_eq!(
            check(&mut s, 14, &[594_000]),
            Err(Error::CdclkTooLow {
                have: 192_000,
                need: 326_700
            })
        );
        s.set(DE_PLL_ENABLE, DE_PLL_ON);
        assert!(matches!(
            check(&mut s, 14, &[148_500]),
            Err(Error::NotSupported(_))
        ));
        // display 9 has no DE PLL register check here and one pixel per clock
        assert_eq!(check(&mut s, 9, &[148_500]), Ok(192_000));
        assert_eq!(
            check(&mut s, 9, &[200_000]),
            Err(Error::CdclkTooLow {
                have: 192_000,
                need: 220_000
            })
        );
        assert_eq!(needed(&[], 14), 0);
    }
}
