//! The DP mode set against a simulated display engine: PHY behind the message bus, a sink behind
//! the AUX channel and the port, power and transcoder registers that answer like the hardware.

use super::*;
use crate::cdclk;
use crate::dp::sink::FakeSink;
use crate::phy::sim_phy;
use crate::plane::XRGB8888;
use crate::sim::{fhd, Sim};
use alloc::boxed::Box;

pub(crate) const TC4: u8 = 6;

/// A Meteor Lake display engine with a DP monitor on TC4 (legacy connector).
pub(crate) fn engine() -> (Sim, alloc::rc::Rc<core::cell::RefCell<FakeSink>>) {
    let mut s = Sim::new();
    let _phy = sim_phy::attach(&mut s, TC4);
    let sink = FakeSink::attach(&mut s, TC4);
    {
        // trained at once: all lanes CR/EQ/locked, aligned
        let mut k = sink.borrow_mut();
        k.dpcd[0x202..0x205].copy_from_slice(&[0x77, 0x77, 0x01]);
    }
    s.set(cdclk::DE_PLL_ENABLE, cdclk::DE_PLL_ON | cdclk::DE_PLL_LOCK);
    s.set(cdclk::CDCLK_CTL, 382);
    s.set(port::tcss_ddi_status(TC4), port::TCSS_READY);
    for r in power::DBUF_CTL {
        s.set(r, power::DBUF_POWER_STATE);
    }
    s.on_write(
        crate::clocks::PMDEMAND1,
        Box::new(|regs, reg, v| {
            regs.insert(reg, v & !(1 << 31));
        }),
    );
    s.set(power::PWR_WELL_CTL2, 0xf);
    s.set(
        crate::init::FUSE_STATUS,
        crate::init::fuse_pg(0) | crate::init::fuse_pg(1),
    );
    s.on_write(
        power::PWR_WELL_CTL2,
        Box::new(|regs, reg, v| {
            regs.insert(reg, v | (v & 0xaaaa_aaaa) >> 1);
        }),
    );
    port_hooks(&mut s);
    (s, sink)
}

/// Port, buffer, transport and transcoder registers that follow their enables like the hardware.
fn port_hooks(s: &mut Sim) {
    // BUF_CTL1 of TC4: SoC ready always; TCSS power state follows the request; D2D state follows enable;
    // the PHY is idle unless the DDI buffer is on
    s.on_write(
        port::buf_ctl1(TC4),
        Box::new(|regs, reg, v| {
            let mut n = v | port::BUF1_SOC_PHY_READY;
            n = if v & port::BUF1_TCSS_POWER_REQUEST != 0 {
                n | port::BUF1_TCSS_POWER_STATE
            } else {
                n & !port::BUF1_TCSS_POWER_STATE
            };
            n = if v & port::BUF1_D2D_ENABLE != 0 {
                n | port::BUF1_D2D_STATE
            } else {
                n & !port::BUF1_D2D_STATE
            };
            let ddi_on = regs.get(&ddi::buf_ctl(TC4)).copied().unwrap_or(0) & ddi::BUF_ENABLE != 0;
            n = if ddi_on {
                n & !port::BUF1_PHY_IDLE
            } else {
                n | port::BUF1_PHY_IDLE
            };
            regs.insert(reg, n);
        }),
    );
    s.set(
        port::buf_ctl1(TC4),
        port::BUF1_SOC_PHY_READY | port::BUF1_PHY_IDLE,
    );
    s.on_write(
        ddi::buf_ctl(TC4),
        Box::new(|regs, _, v| {
            let r = port::buf_ctl1(TC4);
            let b = regs.get(&r).copied().unwrap_or(0);
            regs.insert(
                r,
                if v & ddi::BUF_ENABLE != 0 {
                    b & !port::BUF1_PHY_IDLE
                } else {
                    b | port::BUF1_PHY_IDLE
                },
            );
        }),
    );
    transcoder_hooks(s);
}

/// DP transport idle status and transcoder state follow their enables.
fn transcoder_hooks(s: &mut Sim) {
    s.on_write(
        ddi::dp_tp_ctl(0),
        Box::new(|regs, _, v| {
            let idle = v & ddi::TP_TRAIN_MASK == ddi::TrainPattern::Idle.bits();
            regs.insert(
                ddi::dp_tp_status(0),
                if idle { ddi::TP_STATUS_IDLE_DONE } else { 0 },
            );
        }),
    );
    s.on_write(
        pipe(0, regs::TRANSCONF),
        Box::new(|regs, reg, v| {
            regs.insert(
                reg,
                if v & regs::TRANSCONF_ENABLE != 0 {
                    v | regs::TRANSCONF_STATE
                } else {
                    v & !regs::TRANSCONF_STATE
                },
            );
        }),
    );
}

pub(crate) fn request(caps: dpcd::Caps) -> DpModeset {
    DpModeset {
        pipe: 0,
        port: TC4,
        timing: fhd(),
        plane: PlaneConfig {
            surf: 0x8000_0000,
            pitch: 7680,
            width: 1920,
            height: 1080,
            format: XRGB8888,
        },
        caps,
        bpc: 8,
    }
}

#[test]
fn lights_the_beelink_monitor_like_linux() {
    let (mut s, sink) = engine();
    let caps = read_caps(&mut s, TC4, 14).unwrap();
    assert_eq!(caps.max_rate, 270_000);
    let cfg = enable_dp(&mut s, &request(caps)).unwrap();
    assert_eq!(
        cfg,
        link::LinkConfig {
            rate: 162_000,
            lanes: 4
        }
    );
    // the registers Linux left behind for the same mode (captured dump)
    assert_eq!(s.get(0x60400), 0xba03_0006);
    assert_eq!(s.get(0x60030), 0x7e58_0000);
    assert_eq!(s.get(0x60034), 0x0080_0000);
    assert_eq!(s.get(0x60040), 0x0007_5555);
    assert_eq!(s.get(0x60044), 0x0008_0000);
    assert_eq!(s.get(0x60000), 0x0897_077f);
    assert_eq!(s.get(0x6001c), 0x077f_0437);
    assert_eq!(s.get(0x46140), 0x9000_0000);
    assert_eq!(s.get(0x70180), 0x8400_0000);
    assert_eq!(s.get(0x7019c), 0x8000_0000);
    assert_eq!(s.get(0x45270), 0x77);
    assert!(s.get(0x70008) & regs::TRANSCONF_STATE != 0);
    assert_eq!(
        s.get(ddi::dp_tp_ctl(0)) & ddi::TP_TRAIN_MASK,
        ddi::TrainPattern::Normal.bits()
    );
    assert!(s.get(port::buf_ctl1(TC4)) & port::BUF1_TC_PHY_OWNERSHIP != 0);
    assert!(pll::enabled(&mut s, TC4));
    let k = sink.borrow();
    assert_eq!(&k.dpcd[0x100..0x103], &[0x06, 0x84, 0x00]);
    assert_eq!(k.dpcd[0x600], 1);
}

#[test]
fn disable_turns_everything_off() {
    let (mut s, sink) = engine();
    let caps = read_caps(&mut s, TC4, 14).unwrap();
    enable_dp(&mut s, &request(caps)).unwrap();
    assert_eq!(
        disable(&mut s, 0, TC4),
        Ok(()),
        "every wait of the disable path is answered"
    );
    assert_eq!(
        s.get(0x70008) & (regs::TRANSCONF_ENABLE | regs::TRANSCONF_STATE),
        0
    );
    assert_eq!(s.get(0x60400), 0);
    assert_eq!(s.get(0x70180), 0);
    assert_eq!(s.get(ddi::buf_ctl(TC4)) & ddi::BUF_ENABLE, 0);
    assert!(!pll::enabled(&mut s, TC4));
    assert_eq!(sink.borrow().dpcd[0x600], 2);
    tc_disconnect(&mut s, TC4);
    assert_eq!(s.get(port::buf_ctl1(TC4)) & port::BUF1_TC_PHY_OWNERSHIP, 0);
}

#[test]
fn failed_training_falls_back_and_cleans_up() {
    let (mut s, sink) = engine();
    let caps = read_caps(&mut s, TC4, 14).unwrap();
    sink.borrow_mut().dpcd[0x202..0x205].copy_from_slice(&[0, 0, 0]);
    let mut big = request(caps);
    big.timing = Timing {
        clock: 297_000,
        ..fhd()
    }; // 1080p120 needs HBR x4
    let r = enable_dp(&mut s, &big);
    assert!(matches!(r, Err(Error::LinkTraining(_))));
    assert_eq!(s.get(ddi::buf_ctl(TC4)) & ddi::BUF_ENABLE, 0);
    assert!(!pll::enabled(&mut s, TC4));
}

#[test]
fn refusals() {
    let (mut s, _sink) = engine();
    let caps = read_caps(&mut s, TC4, 14).unwrap();
    let mut a = request(caps);
    a.port = 1;
    assert_eq!(
        enable_dp(&mut s, &a),
        Err(Error::NotSupported("mode set on ports A/B (C10 PHY)"))
    );
    let mut fast = request(caps);
    fast.timing = Timing {
        clock: 594_000,
        hdisplay: 3840,
        hsync_start: 4016,
        hsync_end: 4104,
        htotal: 4400,
        vdisplay: 2160,
        vsync_start: 2168,
        vsync_end: 2178,
        vtotal: 2250,
        flags: 5,
    };
    s.set(0x71008, regs::TRANSCONF_ENABLE); // pipe B runs: CDCLK may not change
    assert!(matches!(
        enable_dp(&mut s, &fast),
        Err(Error::CdclkTooLow { .. })
    ));
    let mut bad = request(caps);
    bad.timing.hsync_end = 1000;
    assert_eq!(enable_dp(&mut s, &bad), Err(Error::Invalid("timing order")));
    s.set(port::tcss_ddi_status(TC4), 0);
    assert!(matches!(
        tc_connect(&mut s, TC4),
        Err(Error::Timeout { .. })
    ));
    assert_eq!(
        s.get(port::buf_ctl1(TC4)) & (port::BUF1_TCSS_POWER_REQUEST | port::BUF1_TC_PHY_OWNERSHIP),
        0,
        "a failed connect gives the PHY back"
    );
    assert_eq!(tc_connect(&mut s, 1), Ok(BOTH_LANES));
}

#[test]
fn cold_display_core_is_brought_up_first() {
    let (mut s, _sink) = engine();
    let caps = read_caps(&mut s, TC4, 14).unwrap();
    // state after a function level reset: no wells, DE PLL off, DBUF off
    s.set(power::PWR_WELL_CTL2, 0);
    s.set(cdclk::DE_PLL_ENABLE, 0);
    s.set(cdclk::CDCLK_CTL, 0);
    for r in power::DBUF_CTL {
        s.set(r, 0);
        s.on_write(
            r,
            Box::new(|regs, reg, v| {
                regs.insert(reg, v | power::DBUF_POWER_STATE);
            }),
        );
    }
    s.on_write(
        cdclk::DE_PLL_ENABLE,
        Box::new(|regs, reg, v| {
            regs.insert(
                reg,
                if v & cdclk::DE_PLL_ON != 0 {
                    v | cdclk::DE_PLL_LOCK
                } else {
                    v & !cdclk::DE_PLL_LOCK
                },
            );
        }),
    );
    s.on_write(
        power::DBUF_CTL_S1,
        Box::new(|regs, reg, v| {
            regs.insert(reg, v | power::DBUF_POWER_STATE);
        }),
    );
    s.on_write(
        crate::clocks::PMDEMAND1,
        Box::new(|regs, reg, v| {
            regs.insert(reg, v & !(1 << 31));
        }),
    );
    enable_dp(&mut s, &request(caps)).unwrap();
    assert_eq!(s.get(cdclk::CDCLK_CTL), 0x0038_017e);
    assert!(s.get(0x70008) & regs::TRANSCONF_STATE != 0);
    assert_eq!(s.get(power::PWR_WELL_CTL2) & 0xf, 0xf);
}
