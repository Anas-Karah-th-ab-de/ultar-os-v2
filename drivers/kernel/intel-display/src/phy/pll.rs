//! Enabling and disabling the C20 PHY PLL of a Type-C port for DisplayPort (steps 1–10 of
//! Intel's "CX0 PLL enable" sequence, in the order of Linux' `intel_cx0pll_enable`, MIT), and the
//! drive levels during link training.

use super::c20;
use super::cx0::{self, BOTH_LANES};
use crate::dp::training::{level_index, Level};
use crate::error::{Error, Result};
use crate::hw::Hw;
use crate::regs::{ddi, port};

fn powerdown_state_bits(mask: u8, state: u8) -> u32 {
    (0..2)
        .filter(|l| mask & (1 << l) != 0)
        .fold(0, |v, l| v | port::buf2_powerdown_state(l, state))
}

fn powerdown_update_bits(mask: u8) -> u32 {
    (0..2)
        .filter(|l| mask & (1 << l) != 0)
        .fold(0, |v, l| v | port::buf2_powerdown_update(l))
}

/// Moves the lanes of `mask` to power state `state` and waits for the update.
pub fn powerdown_change<H: Hw>(hw: &mut H, p: u8, mask: u8, state: u8) -> Result<()> {
    let r = port::buf_ctl2(p);
    hw.rmw(
        r,
        powerdown_state_bits(BOTH_LANES, 0xf),
        powerdown_state_bits(mask, state),
    );
    for lane in 0..2 {
        if mask & (1 << lane) != 0
            && hw
                .wait_clear(port::msgbus_ctl(p, lane), port::M2P_PENDING, 1000)
                .is_err()
        {
            hw.write(port::msgbus_ctl(p, lane), port::M2P_RESET);
        }
    }
    hw.rmw(
        r,
        powerdown_update_bits(BOTH_LANES),
        powerdown_update_bits(mask),
    );
    hw.wait_clear(r, powerdown_update_bits(mask), 2000)
        .map(|_| ())
}

fn lane_reset<H: Hw>(hw: &mut H, p: u8, owned: u8) -> Result<()> {
    let resets = if owned == BOTH_LANES {
        port::buf2_lane_pipe_reset(0) | port::buf2_lane_pipe_reset(1)
    } else {
        port::buf2_lane_pipe_reset(0)
    };
    let status = if owned == BOTH_LANES {
        port::buf2_lane_phy_status(0) | port::buf2_lane_phy_status(1)
    } else {
        port::buf2_lane_phy_status(0)
    };
    hw.wait_set(port::buf_ctl1(p), port::BUF1_SOC_PHY_READY, 1000)?;
    hw.rmw(port::buf_ctl2(p), resets, resets);
    hw.wait_set(port::buf_ctl2(p), status, 1000)?;
    let all_req = port::clk_refclk_request(0) | port::clk_refclk_request(1);
    hw.rmw(port::clock_ctl(p), all_req, port::clk_refclk_request(0));
    let all_ack = port::clk_refclk_ack(0) | port::clk_refclk_ack(1);
    hw.wait(port::clock_ctl(p), all_ack, port::clk_refclk_ack(0), 1000)?;
    powerdown_change(hw, p, BOTH_LANES, port::P2_READY)?;
    hw.rmw(
        port::buf_ctl2(p),
        port::BUF2_POWER_STATE_READY_MASK,
        (port::P2_READY as u32) << 4,
    );
    hw.rmw(
        port::buf_ctl3(p),
        port::BUF3_POWER_STATE_ACTIVE_MASK | port::BUF3_STAGGER_MASK,
        port::P0_ACTIVE as u32,
    );
    hw.rmw(port::buf_ctl2(p), resets, 0);
    hw.wait_clear(port::buf_ctl2(p), status, 15_000).map(|_| ())
}

/// Turns on the transmitters of the used lanes and off the others (no lane reversal).
fn program_lanes<H: Hw>(hw: &mut H, p: u8, owned: u8, lane_count: u8) -> Result<()> {
    let disables: u8 = 0b1111 << lane_count;
    for i in 0..4u8 {
        let mask = if i < 2 { 1 } else { 2 };
        if owned & mask == 0 {
            continue;
        }
        let set = if disables & (1 << i) != 0 { 1 << 6 } else { 0 };
        cx0::rmw(hw, p, mask, c20::tx_control2(i % 2 + 1), 1 << 6, set)?;
    }
    Ok(())
}

/// `DDI_CLK_VALFREQ`: the link symbol clock (port clock / 10 × 8 / 8 for 8b/10b = port clock).
pub fn symbol_clock(rate: u32) -> u32 {
    rate
}

/// Enables the C20 PLL of Type-C port `p` for DP at `rate` on `lane_count` lanes.
pub fn enable_dp<H: Hw>(hw: &mut H, p: u8, owned: u8, rate: u32, lane_count: u8) -> Result<()> {
    let pll = c20::dp_table(rate).ok_or(Error::Invalid("link rate"))?;
    cx0::program_timer(hw, p);
    // 1. clock muxes: no lane reversal, forward clock on, the PHY PLL as DDI clock, no SSC
    hw.rmw(port::buf_ctl1(p), port::BUF1_REVERSAL, 0);
    let mask = port::CLK_LANE1_PHY_SELECT
        | port::CLK_FORWARD_UNGATE
        | port::CLK_DDI_SELECT_MASK
        | port::CLK_SSC_MASK;
    hw.rmw(
        port::clock_ctl(p),
        mask,
        port::CLK_FORWARD_UNGATE | port::CLK_DDI_SELECT_MAXPCLK,
    );
    // 2. PHY out of reset, 3. lanes ready
    lane_reset(hw, p, owned)?;
    powerdown_change(hw, p, BOTH_LANES, port::P2_READY)?;
    // 5. PLL registers, 6. transmitters
    cx0::c20_program(hw, p, owned, pll, rate)?;
    program_lanes(hw, p, owned, lane_count)?;
    // 8. DDI clock frequency, 9./10. PLL request on lane 0 and lock
    hw.write(ddi::clk_valfreq(p), symbol_clock(rate));
    let all = port::clk_pll_request(0) | port::clk_pll_request(1);
    hw.rmw(port::clock_ctl(p), all, port::clk_pll_request(0));
    let acks = port::clk_pll_ack(0) | port::clk_pll_ack(1);
    hw.wait(port::clock_ctl(p), acks, port::clk_pll_ack(0), 3200)
        .map(|_| ())
}

/// PHY power state "P4PG": lanes off (C20 on display 14).
pub const P4PG_DISABLE: u8 = 0xc;

/// Turns the PLL of port `p` off again: lanes to the disable state, PLL and reference clock
/// requests off, DDI clock frequency 0, clocks gated.
pub fn disable<H: Hw>(hw: &mut H, p: u8) -> Result<()> {
    cx0::program_timer(hw, p);
    powerdown_change(hw, p, BOTH_LANES, P4PG_DISABLE)?;
    let req = port::clk_pll_request(0)
        | port::clk_pll_request(1)
        | port::clk_refclk_request(0)
        | port::clk_refclk_request(1);
    hw.rmw(port::clock_ctl(p), req, 0);
    hw.write(ddi::clk_valfreq(p), 0);
    let acks = port::clk_pll_ack(0)
        | port::clk_pll_ack(1)
        | port::clk_refclk_ack(0)
        | port::clk_refclk_ack(1);
    let unlocked = hw.wait_clear(port::clock_ctl(p), acks, 100);
    hw.rmw(port::clock_ctl(p), port::CLK_DDI_SELECT_MASK, 0);
    hw.rmw(port::clock_ctl(p), port::CLK_FORWARD_UNGATE, 0);
    unlocked.map(|_| ())
}

/// Whether the PLL of port `p` runs (PLL acknowledge of lane 0).
pub fn enabled<H: Hw>(hw: &mut H, p: u8) -> bool {
    hw.read(port::clock_ctl(p)) & port::clk_pll_ack(0) != 0
}

/// Reads back the DP rate the C20 vendor registers are set to (`None`: not DP or unknown).
pub fn read_dp_rate<H: Hw>(hw: &mut H, p: u8) -> Result<Option<u32>> {
    cx0::program_timer(hw, p);
    let serdes = cx0::read(hw, p, 0, c20::VDR_SERDES_RATE)?;
    if serdes & c20::IS_DP == 0 {
        return Ok(None);
    }
    Ok(match (serdes & c20::DP_RATE_MASK) >> 1 {
        0 => Some(162_000),
        1 => Some(270_000),
        2 => Some(540_000),
        3 => Some(810_000),
        _ => None,
    })
}

/// Sets the drive level of the first `lane_count` lanes (C20, DP 1.4 rates).
pub fn set_level<H: Hw>(hw: &mut H, p: u8, owned: u8, lane_count: u8, level: Level) -> Result<()> {
    let (swing, pre, post) = c20::C20_DP14_TRANS[level_index(level)];
    for ln in 0..lane_count {
        let lane = ln / 2;
        let tx = ln % 2;
        if owned & (1 << lane) == 0 {
            continue;
        }
        let m = 1 << lane;
        cx0::rmw(hw, p, m, c20::vdrovrd(lane, tx, 0), 0x3f, pre & 0x3f)?;
        cx0::rmw(hw, p, m, c20::vdrovrd(lane, tx, 1), 0x3f, swing & 0x3f)?;
        cx0::rmw(hw, p, m, c20::vdrovrd(lane, tx, 2), 0x3f, post & 0x3f)?;
    }
    cx0::rmw(hw, p, owned, c20::VDR_OVRD, 0, 0b101)
}
