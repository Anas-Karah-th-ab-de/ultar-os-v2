//! Full mode set of display 14 for DisplayPort single stream on a Type-C port (C20 PHY): Type-C
//! PHY ownership, AUX power, PHY PLL, transcoder routing, link training, timings, M/N, DDB and
//! watermarks, primary plane, transcoder enable – and the reverse. The order of the steps is that
//! of Intel's "Enable sequence for DisplayPort" as implemented by Linux' `mtl_ddi_pre_enable_dp`
//! and `intel_enable_ddi` (MIT).

use crate::dp::{aux, dpcd, link, training};
use crate::error::{Error, Result};
use crate::hw::Hw;
use crate::mode::{self, Timing, FLAG_PHSYNC, FLAG_PVSYNC};
use crate::phy::{cx0::BOTH_LANES, pll};
use crate::plane::{self, PlaneConfig};
use crate::regs::{self, ddi, pipe, port, trans};
use crate::{cdclk, clocks, init, power, wm};

/// Everything a DP mode set needs.
#[derive(Clone, Copy, Debug)]
pub struct DpModeset {
    /// Pipe (= transcoder).
    pub pipe: u8,
    /// Type-C port.
    pub port: u8,
    /// Timing.
    pub timing: Timing,
    /// Primary plane content.
    pub plane: PlaneConfig,
    /// Sink capabilities.
    pub caps: dpcd::Caps,
    /// Bits per colour (8).
    pub bpc: u8,
}

/// `CHICKEN_TRANS` of display 14 (0x604e0 + transcoder) and its FEC jitter workaround bit.
pub fn chicken_trans(t: u8) -> u32 {
    trans(t, 0x4e0)
}
const FEC_BS_JITTER_WA: u32 = 1 << 15;

/// Reads the sink's capabilities (the extended field when the sink has one).
pub fn read_caps<H: Hw>(hw: &mut H, p: u8, ver: u8) -> Result<dpcd::Caps> {
    let mut d = [0u8; 16];
    aux::dpcd_read(hw, p, ver, dpcd::DPCD_REV, &mut d)?;
    if dpcd::Caps::has_extended(&d) {
        let mut e = [0u8; 16];
        if aux::dpcd_read(hw, p, ver, dpcd::EXTENDED_CAPS, &mut e).is_ok() && e[0] != 0 {
            e[0x0e] = d[0x0e];
            d = e;
        }
    }
    dpcd::Caps::parse(&d)
}

/// Takes the Type-C PHY of `p` for the display engine (TCSS power, PHY ready, ownership).
pub fn tc_connect<H: Hw>(hw: &mut H, p: u8) -> Result<u8> {
    if !port::is_tc(p) {
        return Ok(BOTH_LANES);
    }
    hw.rmw(port::buf_ctl1(p), 0, port::BUF1_TCSS_POWER_REQUEST);
    let ready = hw
        .wait(
            port::tcss_ddi_status(p),
            port::TCSS_READY,
            port::TCSS_READY,
            5000,
        )
        .and_then(|_| hw.wait_set(port::buf_ctl1(p), port::BUF1_TCSS_POWER_STATE, 5000));
    if let Err(e) = ready {
        // nothing stays requested when the PHY does not come
        tc_disconnect(hw, p);
        return Err(e);
    }
    hw.rmw(port::buf_ctl1(p), 0, port::BUF1_TC_PHY_OWNERSHIP);
    Ok(BOTH_LANES)
}

/// Gives the Type-C PHY back (ownership and TCSS power off).
pub fn tc_disconnect<H: Hw>(hw: &mut H, p: u8) {
    if port::is_tc(p) {
        hw.rmw(
            port::buf_ctl1(p),
            port::BUF1_TC_PHY_OWNERSHIP | port::BUF1_TCSS_POWER_REQUEST,
            0,
        );
    }
}

/// The link-training side of the hardware for one port and transcoder.
struct HwTrainer<'a, H: Hw> {
    hw: &'a mut H,
    port: u8,
    trans: u8,
    lanes: u8,
    owned: u8,
    ver: u8,
}

impl<H: Hw> training::Trainer for HwTrainer<'_, H> {
    fn source_pattern(&mut self, p: ddi::TrainPattern) -> Result<()> {
        self.hw
            .rmw(ddi::dp_tp_ctl(self.trans), ddi::TP_TRAIN_MASK, p.bits());
        if p == ddi::TrainPattern::Idle {
            self.hw.wait_set(
                ddi::dp_tp_status(self.trans),
                ddi::TP_STATUS_IDLE_DONE,
                2000,
            )?;
        }
        Ok(())
    }
    fn source_level(&mut self, level: training::Level) -> Result<()> {
        pll::set_level(self.hw, self.port, self.owned, self.lanes, level)
    }
    fn dpcd_write(&mut self, addr: u32, data: &[u8]) -> Result<()> {
        aux::dpcd_write(self.hw, self.port, self.ver, addr, data)
    }
    fn dpcd_read(&mut self, addr: u32, buf: &mut [u8]) -> Result<()> {
        aux::dpcd_read(self.hw, self.port, self.ver, addr, buf)
    }
    fn delay_us(&mut self, us: u32) {
        self.hw.delay_us(us);
    }
}

/// Port side up to and including link training (6.a–6.n of the sequence).
fn enable_link<H: Hw>(hw: &mut H, m: &DpModeset, cfg: link::LinkConfig, owned: u8) -> Result<()> {
    let (p, t) = (m.port, m.pipe);
    pll::enable_dp(hw, p, owned, cfg.rate, cfg.lanes)?;
    hw.write(ddi::clk_sel(t), ddi::clk_sel_bits(port::phy(p)));
    let mut func = ddi::port_bits_tgl(p)
        | ddi::DdiMode::DpSst.bits()
        | ddi::bpc_bits(m.bpc).ok_or(Error::Invalid("bpc"))?;
    func |= (cfg.lanes as u32 - 1) << 1;
    if m.timing.flags & FLAG_PHSYNC != 0 {
        func |= ddi::FUNC_PHSYNC;
    }
    if m.timing.flags & FLAG_PVSYNC != 0 {
        func |= ddi::FUNC_PVSYNC;
    }
    hw.write(ddi::func_ctl(t), func);
    aux::dpcd_write(hw, p, 14, dpcd::SET_POWER, &[1])?;
    let mut tp = ddi::TP_ENABLE | ddi::TrainPattern::Tps1.bits();
    if m.caps.enhanced_framing {
        tp |= ddi::TP_ENHANCED_FRAMING;
    }
    hw.write(ddi::dp_tp_ctl(t), tp);
    hw.rmw(port::buf_ctl1(p), 0, port::BUF1_D2D_ENABLE);
    hw.wait_set(port::buf_ctl1(p), port::BUF1_D2D_STATE, 100)?;
    pll::set_level(hw, p, owned, cfg.lanes, (0, 0))?;
    hw.rmw(
        port::buf_ctl1(p),
        port::BUF1_WIDTH_MASK | port::BUF1_DATA_WIDTH_MASK,
        port::buf1_width_bits(cfg.lanes),
    );
    hw.write(
        ddi::buf_ctl(p),
        ddi::BUF_ENABLE | ddi::buf_width_bits(cfg.lanes),
    );
    hw.wait_clear(port::buf_ctl1(p), port::BUF1_PHY_IDLE, 10_000)?;
    let mut tr = HwTrainer {
        hw,
        port: p,
        trans: t,
        lanes: cfg.lanes,
        owned,
        ver: 14,
    };
    training::train(&mut tr, cfg, &m.caps)?;
    hw.rmw(
        ddi::dp_tp_ctl(t),
        ddi::TP_TRAIN_MASK,
        ddi::TrainPattern::Normal.bits(),
    );
    Ok(())
}

/// Pipe side: timings, M/N, DDB/watermarks, plane, transcoder enable.
fn enable_pipe<H: Hw>(hw: &mut H, m: &DpModeset, cfg: link::LinkConfig) -> Result<()> {
    let t = m.pipe;
    let mn = link::compute_mn(m.timing.clock, m.bpc as u32 * 3, cfg);
    mode::write_regs(hw, t, &m.timing, 14);
    hw.write(trans(t, regs::DATA_M1), ((mn.tu - 1) << 25) | mn.data_m);
    hw.write(trans(t, regs::DATA_N1), mn.data_n);
    hw.write(trans(t, regs::LINK_M1), mn.link_m);
    hw.write(trans(t, regs::LINK_N1), mn.link_n);
    hw.write(ddi::msa_misc(t), ddi::msa_misc_bits(m.bpc));
    hw.write(pipe(t, regs::PIPE_MISC), 0);
    let lat = wm::read_latency(hw);
    let cpp = plane::bytes_per_pixel(m.plane.format).ok_or(Error::Invalid("pixel format"))?;
    let pw = wm::level0(
        m.timing.clock,
        m.timing.htotal as u32,
        m.plane.width,
        cpp,
        lat,
    )?;
    let cw = wm::level0(m.timing.clock, m.timing.htotal as u32, 256, 4, lat)?;
    wm::program(
        hw,
        t,
        if t.is_multiple_of(2) {
            0
        } else {
            wm::PIPE_BLOCKS
        },
        &pw,
        &cw,
        m.timing.clock,
        m.timing.htotal as u32,
    )?;
    plane::enable(hw, t, 0, &m.plane, 14)?;
    hw.rmw(chicken_trans(t), 0, FEC_BS_JITTER_WA);
    hw.rmw(ddi::func_ctl(t), 0, ddi::FUNC_ENABLE);
    hw.write(pipe(t, regs::TRANSCONF), regs::TRANSCONF_ENABLE);
    hw.wait_set(pipe(t, regs::TRANSCONF), regs::TRANSCONF_STATE, 100_000)
        .map(|_| ())
}

/// Whether a pipe other than `p` runs.
fn others_running<H: Hw>(hw: &mut H, p: u8) -> bool {
    (0..4u8)
        .filter(|q| *q != p)
        .any(|q| hw.read(pipe(q, regs::TRANSCONF)) & regs::TRANSCONF_ENABLE != 0)
}

/// Checks a mode set before anything is touched: timing, plane, port kind, a link that carries
/// the mode, and a CDCLK that serves it (raising CDCLK needs all other pipes off). Returns the
/// first link config to train.
pub fn precheck<H: Hw>(hw: &mut H, m: &DpModeset) -> Result<link::LinkConfig> {
    m.timing.validate()?;
    m.plane.validate()?;
    if !port::is_tc(m.port) {
        return Err(Error::NotSupported("mode set on ports A/B (C10 PHY)"));
    }
    let need = cdclk::needed(&[m.timing.clock], 14);
    init_choose_ok(need)?;
    if others_running(hw, m.pipe) {
        let have = cdclk::read(hw, 14).ok_or(Error::NotSupported("CDCLK off while a pipe runs"))?;
        if have < need {
            return Err(Error::CdclkTooLow { have, need });
        }
    }
    link::choose(
        m.timing.clock,
        m.bpc as u32 * 3,
        m.caps.max_rate.min(810_000),
        m.caps.max_lanes,
    )
}

fn init_choose_ok(need: u32) -> Result<()> {
    clocks::choose(need).map(|_| ())
}

/// Display core, power wells, DBUF slices and PM demand for the mode set.
fn prepare_core<H: Hw>(hw: &mut H, m: &DpModeset) -> Result<()> {
    let others = others_running(hw, m.pipe);
    init::core_init(hw, m.timing.clock, others)?;
    for w in power::wells_for_pipe(m.pipe) {
        power::enable(hw, w)?;
    }
    let base = if m.pipe.is_multiple_of(2) {
        0
    } else {
        wm::PIPE_BLOCKS
    };
    let slices = power::dbuf_slices_of(base, base + wm::PIPE_BLOCKS);
    for s in (0..4u8).filter(|s| slices & (1 << s) != 0) {
        power::dbuf_on(hw, s)?;
    }
    clocks::pmdemand_activity(hw, 1, 1, 1, m.timing.clock.max(162_000).div_ceil(1000))
}

/// Trains from `first` on; after a failure the next config follows the DP fallback order
/// (lower rate, then fewer lanes at the sink's highest rate).
fn train_with_fallback<H: Hw>(
    hw: &mut H,
    m: &DpModeset,
    first: link::LinkConfig,
    owned: u8,
) -> Result<link::LinkConfig> {
    let bpp = m.bpc as u32 * 3;
    let sink = m.caps.max_rate.min(810_000);
    let mut limits = link::Limits {
        max_rate: sink,
        max_lanes: m.caps.max_lanes,
    };
    let mut cfg = first;
    loop {
        match enable_link(hw, m, cfg, owned) {
            Ok(()) => return Ok(cfg),
            Err(e) => {
                let _ = disable_port(hw, m.pipe, m.port);
                match link::next_after_failure(cfg, &mut limits, sink, m.timing.clock, bpp) {
                    Some(n) => cfg = n,
                    None => return Err(e),
                }
            }
        }
    }
}

/// Lights `m.port` with `m.timing` on `m.pipe`, showing `m.plane`. Link training falls back as DP
/// prescribes. On error the pipe and port are off again and the Type-C PHY is given back.
pub fn enable_dp<H: Hw>(hw: &mut H, m: &DpModeset) -> Result<link::LinkConfig> {
    let first = precheck(hw, m)?;
    prepare_core(hw, m)?;
    let owned = tc_connect(hw, m.port)?;
    aux::power_on(hw, m.port);
    let r =
        train_with_fallback(hw, m, first, owned).and_then(|cfg| match enable_pipe(hw, m, cfg) {
            Ok(()) => Ok(cfg),
            Err(e) => {
                let _ = disable(hw, m.pipe, m.port);
                Err(e)
            }
        });
    if r.is_err() {
        tc_disconnect(hw, m.port);
    }
    r
}

/// Result of a failed switch: the error and, if the old mode had to be turned off, the outcome of
/// lighting it again.
pub type SwitchError = (Error, Option<Result<link::LinkConfig>>);

/// Replaces `old` (what runs now, if anything) by `new`. Nothing is touched when `new` fails the
/// checks; when it fails later, `old` is lit again (rollback) and its outcome is returned too.
pub fn switch_dp<H: Hw>(
    hw: &mut H,
    old: Option<&DpModeset>,
    new: &DpModeset,
) -> core::result::Result<link::LinkConfig, SwitchError> {
    precheck(hw, new).map_err(|e| (e, None))?;
    if let Some(o) = old {
        let _ = disable(hw, o.pipe, o.port);
        if o.port != new.port {
            tc_disconnect(hw, o.port);
        }
    }
    enable_dp(hw, new).map_err(|e| (e, old.map(|o| enable_dp(hw, o))))
}

/// Port side off: DDI buffer, transport, D2D link, clock select, PLL. Every step runs; the first
/// error (a wait that timed out) is returned.
fn disable_port<H: Hw>(hw: &mut H, t: u8, p: u8) -> Result<()> {
    hw.rmw(ddi::buf_ctl(p), ddi::BUF_ENABLE, 0);
    let idle = hw
        .wait_set(port::buf_ctl1(p), port::BUF1_PHY_IDLE, 10_000)
        .map(|_| ());
    hw.rmw(ddi::dp_tp_ctl(t), ddi::TP_ENABLE, 0);
    hw.rmw(port::buf_ctl1(p), port::BUF1_D2D_ENABLE, 0);
    let d2d = hw
        .wait_clear(port::buf_ctl1(p), port::BUF1_D2D_STATE, 100)
        .map(|_| ());
    hw.write(ddi::func_ctl(t), 0);
    hw.write(ddi::clk_sel(t), 0);
    let pll = pll::disable(hw, p);
    idle.and(d2d).and(pll)
}

/// Turns pipe `t` and its port `p` off (plane, transcoder, port, PLL; the sink goes to D3). Every
/// step runs; the first error is returned.
pub fn disable<H: Hw>(hw: &mut H, t: u8, p: u8) -> Result<()> {
    plane::disable(hw, t, 0);
    hw.rmw(pipe(t, regs::TRANSCONF), regs::TRANSCONF_ENABLE, 0);
    let off = hw
        .wait_clear(pipe(t, regs::TRANSCONF), regs::TRANSCONF_STATE, 100_000)
        .map(|_| ());
    hw.rmw(ddi::func_ctl(t), ddi::FUNC_ENABLE, 0);
    let d3 = aux::dpcd_write(hw, p, 14, dpcd::SET_POWER, &[2]);
    let port_off = disable_port(hw, t, p);
    off.and(d3).and(port_off)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_switch;
