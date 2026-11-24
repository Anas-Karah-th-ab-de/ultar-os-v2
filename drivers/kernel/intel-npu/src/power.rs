//! Power-up of the NPU 3720 to a running IP block, and power-down.
//!
//! Up: clock off (the power unit may hold a stale request) → leave D0i3 → clock on → the IP
//! block answers → host subsystem out of reset, interconnect quiescent → idle generator off →
//! power island of the NPU's processor on (trickle first) → clocks, isolation off, resets
//! released → host interconnect and top interconnect leave the quiescent state.
//! Down: reset of the IP → clock off.
//!
//! After [`up`] the IP registers are readable ([`hw_version`]); the NPU's processor does not run
//! yet – it starts when the firmware is loaded and its entry point is written (stage N1).

use crate::hw::{field, Hw};
use crate::pll::{self, Ratios};
use crate::regs::*;
use crate::Result;

/// Time limit of the buttress handshakes (150 ms).
pub const TIMEOUT_US: u32 = 150_000;
/// Time limit for the power island (5 ms).
pub const PWR_ISLAND_TIMEOUT_US: u32 = 5_000;
/// Time limit for the IP block to answer after the clock came up (100 µs).
pub const IP_BAR_TIMEOUT_US: u32 = 100;

/// What [`up`] reports besides success.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct UpReport {
    /// The clock-off request before power-up failed (not fatal: it only clears a stale request).
    pub wp0_failed: bool,
    /// Leaving D0i3 failed (not fatal: the following steps show whether the NPU is usable).
    pub d0i3_failed: bool,
}

/// Enters (`true`) or leaves (`false`) the low-power state D0i3.
pub fn d0i3(hw: &mut impl Hw, enter: bool) -> Result<()> {
    hw.poll(
        "D0i3 (before the change)",
        VPU_D0I3_CONTROL,
        VPU_D0I3_INPROGRESS,
        0,
        TIMEOUT_US,
    )?;
    hw.rmw(
        VPU_D0I3_CONTROL,
        VPU_D0I3_I3,
        if enter { VPU_D0I3_I3 } else { 0 },
    );
    hw.poll(
        "D0i3 (after the change)",
        VPU_D0I3_CONTROL,
        VPU_D0I3_INPROGRESS,
        0,
        TIMEOUT_US,
    )?;
    if enter {
        // the NPU needs 5 µs to finish the transition
        hw.delay_us(5);
    }
    Ok(())
}

fn host_ss_configure(hw: &mut impl Hw) -> Result<()> {
    hw.poll(
        "NPU IP block",
        HOST_SS_CPR_RST_CLR,
        CPR_AON,
        0,
        IP_BAR_TIMEOUT_US,
    )?;
    hw.write(HOST_SS_CPR_RST_CLR, CPR_TOP_NOC | CPR_DSS_MAS | CPR_MSS_MAS);
    hw.check("host NOC QREQN", HOST_SS_NOC_QREQN, NOC_TOP_SOCMMIO, 0)?;
    hw.check(
        "host NOC QACCEPTN",
        HOST_SS_NOC_QACCEPTN,
        NOC_TOP_SOCMMIO,
        0,
    )?;
    hw.check("host NOC QDENY", HOST_SS_NOC_QDENY, NOC_TOP_SOCMMIO, 0)
}

fn pwr_domain_enable(hw: &mut impl Hw) -> Result<()> {
    hw.rmw(AON_PWR_ISLAND_TRICKLE_EN0, 0, PWR_MSS_CPU);
    // at least 500 ns between pre-charge and the main switch
    hw.delay_us(1);
    hw.rmw(AON_PWR_ISLAND_EN0, 0, PWR_MSS_CPU);
    hw.poll(
        "power island",
        AON_PWR_ISLAND_STATUS0,
        PWR_MSS_CPU,
        PWR_MSS_CPU,
        PWR_ISLAND_TIMEOUT_US,
    )?;
    let ports = TOP_NOC_CPU_CTRL | TOP_NOC_HOSTIF_L2CACHE;
    hw.check("top NOC QREQN", TOP_NOC_QREQN, ports, 0)?;
    let parts = CPR_TOP_NOC | CPR_DSS_MAS | CPR_MSS_MAS;
    hw.rmw(HOST_SS_CPR_CLK_SET, 0, parts);
    hw.rmw(AON_PWR_ISO_EN0, PWR_MSS_CPU, 0);
    hw.rmw(HOST_SS_CPR_RST_SET, 0, parts);
    hw.rmw(AON_DPU_ACTIVE, 0, DPU_ACTIVE);
    Ok(())
}

fn host_ss_axi_enable(hw: &mut impl Hw) -> Result<()> {
    hw.rmw(HOST_SS_NOC_QREQN, 0, NOC_TOP_SOCMMIO);
    hw.check(
        "host NOC QACCEPTN",
        HOST_SS_NOC_QACCEPTN,
        NOC_TOP_SOCMMIO,
        NOC_TOP_SOCMMIO,
    )?;
    hw.check("host NOC QDENY", HOST_SS_NOC_QDENY, NOC_TOP_SOCMMIO, 0)
}

fn top_noc_enable(hw: &mut impl Hw) -> Result<()> {
    let ports = TOP_NOC_CPU_CTRL | TOP_NOC_HOSTIF_L2CACHE;
    hw.rmw(TOP_NOC_QREQN, 0, ports);
    hw.check("top NOC QACCEPTN", TOP_NOC_QACCEPTN, ports, ports)?;
    hw.check("top NOC QDENY", TOP_NOC_QDENY, ports, 0)
}

/// Powers the NPU up to a running IP block.
pub fn up(hw: &mut impl Hw, ratios: &Ratios) -> Result<UpReport> {
    let report = UpReport {
        wp0_failed: pll::drive(hw, ratios, false).is_err(),
        d0i3_failed: d0i3(hw, false).is_err(),
    };
    pll::drive(hw, ratios, true)?;
    host_ss_configure(hw)?;
    hw.rmw(AON_VPU_IDLE_GEN, VPU_IDLE_GEN_EN, 0);
    pwr_domain_enable(hw)?;
    host_ss_axi_enable(hw)?;
    top_noc_enable(hw)?;
    Ok(report)
}

/// Resets the IP (its registers stop answering) without touching the clock.
pub fn ip_reset(hw: &mut impl Hw) -> Result<()> {
    hw.poll(
        "IP reset (idle)",
        VPU_IP_RESET,
        VPU_IP_RESET_TRIGGER,
        0,
        TIMEOUT_US,
    )?;
    hw.rmw(VPU_IP_RESET, 0, VPU_IP_RESET_TRIGGER);
    hw.poll(
        "IP reset (done)",
        VPU_IP_RESET,
        VPU_IP_RESET_TRIGGER,
        0,
        TIMEOUT_US,
    )?;
    Ok(())
}

/// Powers the NPU down: reset of the IP, then clock off. Both steps run; the first error is
/// returned.
pub fn down(hw: &mut impl Hw, ratios: &Ratios) -> Result<()> {
    let reset = ip_reset(hw);
    let clock = pll::drive(hw, ratios, false);
    reset.and(clock)
}

/// The hardware version of a powered NPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HwVersion {
    /// Generation field (not the IP generation: the NPU 3720 reports `0x36`).
    pub generation: u32,
    /// SoC number.
    pub soc: u32,
    /// SoC revision.
    pub revision: u32,
}

impl HwVersion {
    /// Generation and SoC number as one number, comparable with [`crate::ident::Model::sku`].
    pub fn sku(&self) -> u32 {
        self.generation << 8 | self.soc
    }
}

/// Reads the hardware version (IP block: only after [`up`]).
pub fn hw_version(hw: &mut impl Hw) -> Option<HwVersion> {
    let v = hw.read(HOST_SS_HW_VERSION);
    (v != u32::MAX).then(|| HwVersion {
        generation: field(v, HW_VERSION_VPU_GENERATION),
        soc: field(v, HW_VERSION_SOC_NUMBER),
        revision: field(v, HW_VERSION_SOC_REVISION),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::pll::tests::power_unit;
    use crate::sim::Sim;
    use crate::Error;
    use alloc::boxed::Box;

    /// An NPU whose handshakes all answer at once.
    pub fn npu() -> Sim {
        let mut s = Sim::new();
        power_unit(&mut s);
        s.on_write(
            AON_PWR_ISLAND_EN0,
            Box::new(|r, _, v| {
                r.insert(AON_PWR_ISLAND_STATUS0, v & PWR_MSS_CPU);
            }),
        );
        s.on_write(
            HOST_SS_NOC_QREQN,
            Box::new(|r, _, v| {
                r.insert(HOST_SS_NOC_QACCEPTN, v);
            }),
        );
        s.on_write(
            TOP_NOC_QREQN,
            Box::new(|r, _, v| {
                r.insert(TOP_NOC_QACCEPTN, v);
            }),
        );
        s.on_write(
            VPU_IP_RESET,
            Box::new(|r, reg, _| {
                r.insert(reg, 0);
            }),
        );
        // the island starts isolated, the reset-clear register is write-only (reads 0)
        s.set(AON_PWR_ISO_EN0, PWR_MSS_CPU);
        s.on_write(
            HOST_SS_CPR_RST_CLR,
            Box::new(|r, reg, _| {
                r.insert(reg, 0);
            }),
        );
        // as read on the Beelink GTi15 Ultra
        s.set(HOST_SS_HW_VERSION, 0x0036_3000);
        s
    }

    #[test]
    fn power_up_in_order() {
        let mut s = npu();
        let r = Ratios::read(&mut s);
        assert_eq!(up(&mut s, &r), Ok(UpReport::default()));
        // clock: off first (stale request), then on
        assert_eq!(s.writes_to(WP_REQ_PAYLOAD1), [0x0080_0000, 0x0080_001c]);
        assert_eq!(s.writes_to(VPU_D0I3_CONTROL), [0]);
        assert_eq!(s.writes_to(HOST_SS_CPR_RST_CLR), [0xc02]);
        assert_eq!(s.writes_to(AON_VPU_IDLE_GEN), [0]);
        assert_eq!(s.writes_to(AON_PWR_ISLAND_TRICKLE_EN0), [8]);
        assert_eq!(s.writes_to(AON_PWR_ISLAND_EN0), [8]);
        assert_eq!(s.writes_to(HOST_SS_CPR_CLK_SET), [0xc02]);
        assert_eq!(s.writes_to(AON_PWR_ISO_EN0), [0]);
        assert_eq!(s.writes_to(HOST_SS_CPR_RST_SET), [0xc02]);
        assert_eq!(s.writes_to(AON_DPU_ACTIVE), [1]);
        assert_eq!(s.writes_to(HOST_SS_NOC_QREQN), [1]);
        assert_eq!(s.writes_to(TOP_NOC_QREQN), [3]);
        let order = [
            VPU_D0I3_CONTROL,
            HOST_SS_CPR_RST_CLR,
            AON_VPU_IDLE_GEN,
            AON_PWR_ISLAND_TRICKLE_EN0,
            AON_PWR_ISLAND_EN0,
            HOST_SS_CPR_CLK_SET,
            AON_PWR_ISO_EN0,
            HOST_SS_CPR_RST_SET,
            AON_DPU_ACTIVE,
            HOST_SS_NOC_QREQN,
            TOP_NOC_QREQN,
        ];
        let pos: alloc::vec::Vec<_> = order.iter().map(|r| s.first_write(*r).unwrap()).collect();
        assert!(pos.windows(2).all(|w| w[0] < w[1]), "{pos:?}");
        let v = hw_version(&mut s).unwrap();
        assert_eq!((v.generation, v.soc, v.revision), (0x36, 0x30, 0));
        assert_eq!(v.sku(), crate::ident::lookup(0x7d1d).unwrap().sku);
    }

    #[test]
    fn power_down_resets_then_stops_the_clock() {
        let mut s = npu();
        let r = Ratios::read(&mut s);
        up(&mut s, &r).unwrap();
        s.writes.clear();
        down(&mut s, &r).unwrap();
        assert_eq!(s.writes_to(VPU_IP_RESET), [1]);
        assert_eq!(s.writes_to(WP_REQ_PAYLOAD1), [0x0080_0000]);
        assert!(s.first_write(VPU_IP_RESET) < s.first_write(WP_REQ_CMD));
        assert_eq!(s.get(VPU_STATUS), 0);
    }

    #[test]
    fn a_failed_handshake_stops_the_sequence() {
        // the island never reports power
        let mut s = npu();
        s.on_write(AON_PWR_ISLAND_EN0, Box::new(|_, _, _| {}));
        let r = Ratios::read(&mut s);
        assert!(matches!(
            up(&mut s, &r),
            Err(Error::Timeout {
                what: "power island",
                ..
            })
        ));
        assert!(s.writes_to(HOST_SS_CPR_CLK_SET).is_empty());
        // the top interconnect denies
        let mut s = npu();
        s.set(TOP_NOC_QDENY, TOP_NOC_CPU_CTRL);
        assert!(matches!(
            up(&mut s, &r),
            Err(Error::Check {
                what: "top NOC QDENY",
                ..
            })
        ));
        // the IP block stays in reset: nothing of the IP is written
        let mut s = npu();
        s.set(HOST_SS_CPR_RST_CLR, CPR_AON);
        assert!(matches!(
            up(&mut s, &r),
            Err(Error::Timeout {
                what: "NPU IP block",
                ..
            })
        ));
        assert!(s
            .writes
            .iter()
            .all(|(reg, _)| reg.0 == crate::Space::Buttress));
    }

    #[test]
    fn stale_request_and_d0i3_failures_are_reported_not_fatal() {
        let mut s = npu();
        let r = Ratios::read(&mut s);
        // D0i3 stays "in progress": power-up goes on and says so
        s.set(VPU_D0I3_CONTROL, VPU_D0I3_INPROGRESS);
        let rep = up(&mut s, &r).unwrap();
        assert_eq!(
            rep,
            UpReport {
                wp0_failed: false,
                d0i3_failed: true
            }
        );
    }

    #[test]
    fn down_runs_both_steps_and_keeps_the_first_error() {
        let mut s = npu();
        let r = Ratios::read(&mut s);
        up(&mut s, &r).unwrap();
        s.on_write(VPU_IP_RESET, Box::new(|_, _, _| {}));
        s.writes.clear();
        assert!(matches!(
            down(&mut s, &r),
            Err(Error::Timeout {
                what: "IP reset (done)",
                ..
            })
        ));
        assert_eq!(
            s.writes_to(WP_REQ_CMD),
            [1],
            "the clock is still turned off"
        );
    }

    #[test]
    fn d0i3_enter_and_leave() {
        let mut s = Sim::new();
        d0i3(&mut s, true).unwrap();
        d0i3(&mut s, false).unwrap();
        assert_eq!(s.writes_to(VPU_D0I3_CONTROL), [4, 0]);
    }
}
