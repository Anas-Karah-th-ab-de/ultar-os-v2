//! The NPU's clock: frequency fuses and the *work point* request. The driver does not program
//! the PLL itself; it asks the processor's power unit for a ratio and a configuration and waits
//! until the PLL is locked and the NPU reports ready.

use crate::hw::{field, prep, Hw};
use crate::regs::*;
use crate::Result;

/// How long the power unit may take (1.5 s).
pub const PLL_TIMEOUT_US: u32 = 1_500_000;
/// Reference clock of the PLL in MHz.
const REF_CLK_MHZ: u32 = 50;
/// Energy/performance preference sent with every request.
const EPP_DEFAULT: u32 = 0x80;
/// Work point configuration: two tiles, PLL divider 4/3.
const CONFIG_2_TILE_RATIO_4_3: u32 = 0x02 << 8 | 0x02;

/// The PLL ratios the fuses allow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ratios {
    /// Lowest ratio.
    pub min: u32,
    /// Highest ratio.
    pub max: u32,
    /// Nominal ratio the NPU starts with.
    pub pn: u32,
}

impl Ratios {
    /// Reads the fuses (the nominal ratio is kept inside `min..=max`).
    pub fn read(hw: &mut impl Hw) -> Ratios {
        let fmin = hw.read(FMIN_FUSE);
        let fmax = hw.read(FMAX_FUSE);
        let min = field(fmin, FMIN_FUSE_MIN_RATIO);
        let max = field(fmax, FMAX_FUSE_MAX_RATIO).max(min);
        Ratios {
            min,
            max,
            pn: field(fmin, FMIN_FUSE_PN_RATIO).clamp(min, max),
        }
    }
}

/// Frequency of a ratio in MHz (reference × ratio × 2/3 on this generation).
pub fn ratio_to_mhz(ratio: u32) -> u32 {
    ratio * REF_CLK_MHZ * 2 / 3
}

fn sync(hw: &mut impl Hw, what: &'static str) -> Result<()> {
    hw.poll(what, WP_REQ_CMD, WP_REQ_CMD_SEND, 0, PLL_TIMEOUT_US)
        .map(|_| ())
}

/// Requests the work point: `enable` asks for the nominal ratio with the two-tile configuration,
/// otherwise for ratio 0 (clock off). Waits for the PLL and for the NPU's ready state to follow.
pub fn drive(hw: &mut impl Hw, ratios: &Ratios, enable: bool) -> Result<()> {
    let (target, config) = if enable {
        (ratios.pn, CONFIG_2_TILE_RATIO_4_3)
    } else {
        (0, 0)
    };
    sync(hw, "work point channel (before the request)")?;
    hw.rmw(
        WP_REQ_PAYLOAD0,
        WP_MIN_RATIO | WP_MAX_RATIO,
        prep(WP_MIN_RATIO, ratios.min) | prep(WP_MAX_RATIO, ratios.max),
    );
    hw.rmw(
        WP_REQ_PAYLOAD1,
        WP_TARGET_RATIO | WP_EPP,
        prep(WP_TARGET_RATIO, target) | prep(WP_EPP, EPP_DEFAULT),
    );
    hw.rmw(WP_REQ_PAYLOAD2, WP_CONFIG, prep(WP_CONFIG, config));
    hw.rmw(WP_REQ_CMD, 0, WP_REQ_CMD_SEND);
    sync(hw, "work point channel (after the request)")?;
    let lock = if enable { PLL_STATUS_LOCK } else { 0 };
    hw.poll(
        "PLL lock",
        PLL_STATUS,
        PLL_STATUS_LOCK,
        lock,
        PLL_TIMEOUT_US,
    )?;
    let ready = if enable { VPU_STATUS_READY } else { 0 };
    hw.poll(
        "NPU ready status",
        VPU_STATUS,
        VPU_STATUS_READY,
        ready,
        PLL_TIMEOUT_US,
    )?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::sim::Sim;
    use crate::Error;
    use alloc::boxed::Box;

    /// A power unit that takes every request at once: lock and ready follow the target ratio.
    pub fn power_unit(s: &mut Sim) {
        s.set(FMIN_FUSE, 0x1c0b);
        s.set(FMAX_FUSE, 0x27);
        s.on_write(
            WP_REQ_CMD,
            Box::new(|r, reg, v| {
                if v & WP_REQ_CMD_SEND != 0 {
                    r.insert(reg, v & !WP_REQ_CMD_SEND);
                    let target = r.get(&WP_REQ_PAYLOAD1).copied().unwrap_or(0) & WP_TARGET_RATIO;
                    let on = target != 0;
                    r.insert(PLL_STATUS, if on { PLL_STATUS_LOCK } else { 0 });
                    r.insert(
                        VPU_STATUS,
                        if on {
                            VPU_STATUS_READY | VPU_STATUS_IDLE
                        } else {
                            0
                        },
                    );
                    r.insert(CURRENT_PLL, target);
                }
            }),
        );
    }

    #[test]
    fn fuses() {
        let mut s = Sim::new();
        power_unit(&mut s);
        assert_eq!(
            Ratios::read(&mut s),
            Ratios {
                min: 0x0b,
                max: 0x27,
                pn: 0x1c
            }
        );
        // a nominal ratio outside the limits is pulled in
        s.set(FMIN_FUSE, 0x4010);
        assert_eq!(Ratios::read(&mut s).pn, 0x27);
        s.set(FMAX_FUSE, 0x01);
        assert_eq!(
            Ratios::read(&mut s),
            Ratios {
                min: 0x10,
                max: 0x10,
                pn: 0x10
            }
        );
        assert_eq!(ratio_to_mhz(0x1c), 933);
        assert_eq!(ratio_to_mhz(0x27), 1300);
    }

    #[test]
    fn enable_sends_the_nominal_ratio() {
        let mut s = Sim::new();
        power_unit(&mut s);
        let r = Ratios::read(&mut s);
        drive(&mut s, &r, true).unwrap();
        assert_eq!(s.writes_to(WP_REQ_PAYLOAD0), [0x0027_000b]);
        assert_eq!(s.writes_to(WP_REQ_PAYLOAD1), [0x0080_001c]);
        assert_eq!(s.writes_to(WP_REQ_PAYLOAD2), [0x0202]);
        assert_eq!(s.writes_to(WP_REQ_CMD), [1]);
        assert!(s.first_write(WP_REQ_PAYLOAD2) < s.first_write(WP_REQ_CMD));
        assert_eq!(s.get(CURRENT_PLL), 0x1c);
    }

    #[test]
    fn disable_sends_ratio_zero() {
        let mut s = Sim::new();
        power_unit(&mut s);
        let r = Ratios::read(&mut s);
        drive(&mut s, &r, true).unwrap();
        s.writes.clear();
        drive(&mut s, &r, false).unwrap();
        assert_eq!(s.writes_to(WP_REQ_PAYLOAD1), [0x0080_0000]);
        assert_eq!(s.writes_to(WP_REQ_PAYLOAD2), [0]);
        assert_eq!(s.get(VPU_STATUS), 0);
    }

    #[test]
    fn a_stuck_channel_or_pll_times_out() {
        let mut s = Sim::new();
        s.set(WP_REQ_CMD, WP_REQ_CMD_SEND);
        let r = Ratios {
            min: 1,
            max: 2,
            pn: 1,
        };
        assert!(matches!(
            drive(&mut s, &r, true),
            Err(Error::Timeout {
                what: "work point channel (before the request)",
                ..
            })
        ));
        assert!(s.writes.is_empty());
        // request taken, PLL never locks
        let mut s = Sim::new();
        s.on_write(
            WP_REQ_CMD,
            Box::new(|r, reg, _| {
                r.insert(reg, 0);
            }),
        );
        assert!(matches!(
            drive(&mut s, &r, true),
            Err(Error::Timeout {
                what: "PLL lock",
                ..
            })
        ));
        assert!(s.now_us() >= PLL_TIMEOUT_US as u64);
    }
}
