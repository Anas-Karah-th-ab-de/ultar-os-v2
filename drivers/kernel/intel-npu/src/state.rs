//! A read-only look at the NPU through the buttress: what the fuses allow and what state the
//! firmware of the machine (or an earlier driver) left. Writes nothing.

use crate::hw::{field, Hw};
use crate::pll::{ratio_to_mhz, Ratios};
use crate::regs::*;
use crate::{Error, Result};
use core::fmt;

/// What the buttress says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts {
    /// PLL ratios of the fuses.
    pub ratios: Ratios,
    /// The ratio in use (0: clock off).
    pub current_ratio: u32,
    /// The PLL is locked.
    pub pll_locked: bool,
    /// Powered and clocked.
    pub ready: bool,
    /// Nothing running.
    pub idle: bool,
    /// In the low-power state D0i3.
    pub d0i3: bool,
    /// Raw tile fuse.
    pub tile_fuse: u32,
}

/// Reads the buttress.
pub fn read(hw: &mut impl Hw) -> Result<Facts> {
    let status = hw.read(VPU_STATUS);
    let fmin = hw.read(FMIN_FUSE);
    if status == u32::MAX && fmin == u32::MAX {
        return Err(Error::DeviceGone);
    }
    Ok(Facts {
        ratios: Ratios::read(hw),
        current_ratio: field(hw.read(CURRENT_PLL), CURRENT_PLL_RATIO),
        pll_locked: hw.read(PLL_STATUS) & PLL_STATUS_LOCK != 0,
        ready: status & VPU_STATUS_READY != 0,
        idle: status & VPU_STATUS_IDLE != 0,
        d0i3: hw.read(VPU_D0I3_CONTROL) & VPU_D0I3_I3 != 0,
        tile_fuse: hw.read(TILE_FUSE),
    })
}

impl fmt::Display for Facts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PLL {}–{} MHz (nominal {} MHz), now {} MHz{}, {}{}{}",
            ratio_to_mhz(self.ratios.min),
            ratio_to_mhz(self.ratios.max),
            ratio_to_mhz(self.ratios.pn),
            ratio_to_mhz(self.current_ratio),
            if self.pll_locked { " locked" } else { "" },
            if self.ready { "ready" } else { "off" },
            if self.idle { ", idle" } else { "" },
            if self.d0i3 { ", D0i3" } else { "" },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pll;
    use crate::power::tests::npu;
    use crate::sim::Sim;
    use alloc::string::ToString;

    #[test]
    fn off_and_on() {
        let mut s = npu();
        s.set(VPU_D0I3_CONTROL, VPU_D0I3_I3);
        let f = read(&mut s).unwrap();
        assert!(!f.ready && f.d0i3 && f.current_ratio == 0);
        assert_eq!(
            f.to_string(),
            "PLL 366–1300 MHz (nominal 933 MHz), now 0 MHz, off, D0i3"
        );
        assert!(s.writes.is_empty(), "the read-out must not write");
        let r = f.ratios;
        pll::drive(&mut s, &r, true).unwrap();
        let f = read(&mut s).unwrap();
        assert!(f.ready && f.idle && f.pll_locked);
        assert!(f.to_string().contains("now 933 MHz locked, ready, idle"));
    }

    #[test]
    fn a_device_that_is_gone() {
        let mut s = Sim::new();
        s.set(VPU_STATUS, u32::MAX);
        s.set(FMIN_FUSE, u32::MAX);
        assert_eq!(read(&mut s), Err(Error::DeviceGone));
    }
}
