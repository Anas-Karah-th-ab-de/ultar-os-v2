//! Per-port registers of display 14 ("PICA" block): port buffer control with the Type-C PHY
//! ownership, the PHY message bus, the port clock control and the AUX channel. Ports A and B sit in
//! the 0x64000 block, the Type-C ports TC1..TC4 in 0x16F200 + n × 0x200.
//!
//! Offsets as in Linux' `intel_cx0_phy_regs.h` and `intel_dp_aux_regs.h` (MIT).

use crate::hw::genmask;

/// First Type-C port index (PORT_TC1 = PORT_D).
pub const PORT_TC1: u8 = 3;
/// Last Type-C port index on display 14 (TC4 = PORT_G).
pub const PORT_TC4: u8 = 6;

/// Whether `port` is a Type-C port.
pub const fn is_tc(port: u8) -> bool {
    port >= PORT_TC1 && port <= PORT_TC4
}

/// Type-C index 0..=3 of a Type-C port.
pub const fn tc_index(port: u8) -> u8 {
    port - PORT_TC1
}

/// PHY number of `port` on display 14 (A = 0, B = 1, TC1..TC4 = F..I = 5..8).
pub const fn phy(port: u8) -> u8 {
    if is_tc(port) {
        5 + tc_index(port)
    } else {
        port
    }
}

/// Register at `tc_off` in the Type-C block or `ab_off` in the A/B block of `port`.
const fn pick(port: u8, ab_off: u32, tc_off: u32) -> u32 {
    if is_tc(port) {
        0x16f200 + tc_index(port) as u32 * 0x200 + tc_off
    } else {
        0x64000 + port as u32 * 0x100 + ab_off
    }
}

/// `XELPDP_PORT_BUF_CTL1`.
pub const fn buf_ctl1(port: u8) -> u32 {
    pick(port, 0x04, 0x00)
}
/// `XELPDP_PORT_BUF_CTL2`.
pub const fn buf_ctl2(port: u8) -> u32 {
    pick(port, 0x08, 0x04)
}
/// `XELPDP_PORT_BUF_CTL3`.
pub const fn buf_ctl3(port: u8) -> u32 {
    pick(port, 0x0c, 0x08)
}
/// `DP_AUX_CH_CTL`.
pub const fn aux_ctl(port: u8) -> u32 {
    pick(port, 0x10, 0x10)
}
/// `DP_AUX_CH_DATA` register `i` (0..5).
pub const fn aux_data(port: u8, i: u8) -> u32 {
    pick(port, 0x14, 0x14) + i as u32 * 4
}
/// `XELPDP_PORT_M2P_MSGBUS_CTL` of PHY lane `lane` (0, 1).
pub const fn msgbus_ctl(port: u8, lane: u8) -> u32 {
    pick(port, 0x40, 0x40) + lane as u32 * 4
}
/// `XELPDP_PORT_P2M_MSGBUS_STATUS` of PHY lane `lane`.
pub const fn msgbus_status(port: u8, lane: u8) -> u32 {
    msgbus_ctl(port, lane) + 8
}
/// `XELPDP_PORT_MSGBUS_TIMER` of PHY lane `lane`.
pub const fn msgbus_timer(port: u8, lane: u8) -> u32 {
    pick(port, 0xd8, 0x58) + lane as u32 * 4
}
/// `XELPDP_PORT_CLOCK_CTL`.
pub const fn clock_ctl(port: u8) -> u32 {
    pick(port, 0xe0, 0x60)
}
/// `XELPDP_PORT_HOTPLUG_CTL` (Type-C ports only).
pub const fn hotplug_ctl(port: u8) -> u32 {
    pick(port, 0x70, 0x70)
}
/// `TCSS_DDI_STATUS` of a Type-C port.
pub const fn tcss_ddi_status(port: u8) -> u32 {
    0x161500 + tc_index(port) as u32 * 4
}

/// BUF_CTL1: die-to-die link enable / state.
pub const BUF1_D2D_ENABLE: u32 = 1 << 29;
/// BUF_CTL1: die-to-die link state.
pub const BUF1_D2D_STATE: u32 = 1 << 28;
/// BUF_CTL1: the SoC side of the PHY is out of reset.
pub const BUF1_SOC_PHY_READY: u32 = 1 << 24;
/// BUF_CTL1: data width field.
pub const BUF1_DATA_WIDTH_MASK: u32 = genmask(19, 18);
/// BUF_CTL1: lane reversal.
pub const BUF1_REVERSAL: u32 = 1 << 16;
/// BUF_CTL1: IO select Thunderbolt.
pub const BUF1_IO_SELECT_TBT: u32 = 1 << 11;
/// BUF_CTL1: PHY idle.
pub const BUF1_PHY_IDLE: u32 = 1 << 7;
/// BUF_CTL1: the display engine owns the Type-C PHY.
pub const BUF1_TC_PHY_OWNERSHIP: u32 = 1 << 6;
/// BUF_CTL1: TCSS power request.
pub const BUF1_TCSS_POWER_REQUEST: u32 = 1 << 5;
/// BUF_CTL1: TCSS power state.
pub const BUF1_TCSS_POWER_STATE: u32 = 1 << 4;
/// BUF_CTL1: port width field.
pub const BUF1_WIDTH_MASK: u32 = genmask(3, 1);

/// BUF_CTL1 port width field for `lanes`.
pub fn buf1_width_bits(lanes: u8) -> u32 {
    (if lanes == 3 {
        4
    } else {
        lanes.max(1) as u32 - 1
    }) << 1
}

/// BUF_CTL2: pipe reset of PHY lane `lane`.
pub const fn buf2_lane_pipe_reset(lane: u8) -> u32 {
    1 << (31 - lane as u32)
}
/// BUF_CTL2: current reset status of PHY lane `lane`.
pub const fn buf2_lane_phy_status(lane: u8) -> u32 {
    1 << (29 - lane as u32)
}
/// BUF_CTL2: power-down update of PHY lane `lane`.
pub const fn buf2_powerdown_update(lane: u8) -> u32 {
    1 << (25 - lane as u32)
}
/// BUF_CTL2: new power-down state of PHY lane `lane`.
pub const fn buf2_powerdown_state(lane: u8, state: u8) -> u32 {
    ((state as u32) & 0xf) << (20 - 4 * lane as u32)
}
/// BUF_CTL2: field "power state ready".
pub const BUF2_POWER_STATE_READY_MASK: u32 = genmask(7, 4);
/// BUF_CTL3: field "power state active".
pub const BUF3_POWER_STATE_ACTIVE_MASK: u32 = genmask(3, 0);
/// BUF_CTL3: PLL lane staggering delay.
pub const BUF3_STAGGER_MASK: u32 = genmask(15, 8);

/// PHY power state P0 (active).
pub const P0_ACTIVE: u8 = 0x0;
/// PHY power state P2 (ready; also the reset state).
pub const P2_READY: u8 = 0x2;

/// CLOCK_CTL: PCLK PLL request of lane `lane`.
pub const fn clk_pll_request(lane: u8) -> u32 {
    1 << (31 - 4 * lane as u32)
}
/// CLOCK_CTL: PCLK PLL acknowledge of lane `lane`.
pub const fn clk_pll_ack(lane: u8) -> u32 {
    1 << (30 - 4 * lane as u32)
}
/// CLOCK_CTL: reference clock request of lane `lane`.
pub const fn clk_refclk_request(lane: u8) -> u32 {
    1 << (29 - 4 * lane as u32)
}
/// CLOCK_CTL: reference clock acknowledge of lane `lane`.
pub const fn clk_refclk_ack(lane: u8) -> u32 {
    1 << (28 - 4 * lane as u32)
}
/// CLOCK_CTL: DDI clock select field.
pub const CLK_DDI_SELECT_MASK: u32 = genmask(15, 12);
/// CLOCK_CTL: DDI clock select "MAXPCLK" (the PHY PLL).
pub const CLK_DDI_SELECT_MAXPCLK: u32 = 0x8 << 12;
/// CLOCK_CTL: forward clock ungate.
pub const CLK_FORWARD_UNGATE: u32 = 1 << 10;
/// CLOCK_CTL: lane 1 PHY clock select (lane reversal).
pub const CLK_LANE1_PHY_SELECT: u32 = 1 << 8;
/// CLOCK_CTL: spread spectrum PLL A / B.
pub const CLK_SSC_MASK: u32 = 0b11;

/// MSGBUS_CTL: transaction pending.
pub const M2P_PENDING: u32 = 1 << 31;
/// MSGBUS_CTL: command "write uncommitted".
pub const M2P_WRITE_UNCOMMITTED: u32 = 0x1 << 27;
/// MSGBUS_CTL: command "write committed".
pub const M2P_WRITE_COMMITTED: u32 = 0x2 << 27;
/// MSGBUS_CTL: command "read".
pub const M2P_READ: u32 = 0x3 << 27;
/// MSGBUS_CTL: reset the bus.
pub const M2P_RESET: u32 = 1 << 15;
/// STATUS: a response is ready.
pub const P2M_RESPONSE_READY: u32 = 1 << 31;
/// STATUS: command type field.
pub const P2M_COMMAND_MASK: u32 = genmask(30, 27);
/// STATUS: command type "read acknowledge".
pub const P2M_READ_ACK: u32 = 0x4;
/// STATUS: command type "write acknowledge".
pub const P2M_WRITE_ACK: u32 = 0x5;
/// STATUS: data field.
pub const P2M_DATA_MASK: u32 = genmask(23, 16);
/// STATUS: error.
pub const P2M_ERROR: u32 = 1 << 15;
/// MSGBUS_TIMER: the value Intel asks for (0xA000).
pub const MSGBUS_TIMER_VAL: u32 = 0xa000;
/// MSGBUS_TIMER: value field.
pub const MSGBUS_TIMER_MASK: u32 = genmask(23, 0);

/// TCSS_DDI_STATUS: PHY ready.
pub const TCSS_READY: u32 = 1 << 2;

/// AUX_CH_CTL bits.
pub mod aux {
    use crate::hw::genmask;
    /// Start / busy.
    pub const SEND_BUSY: u32 = 1 << 31;
    /// Done (write 1 to clear).
    pub const DONE: u32 = 1 << 30;
    /// Interrupt on done.
    pub const INTERRUPT: u32 = 1 << 29;
    /// Time-out error (write 1 to clear).
    pub const TIME_OUT_ERROR: u32 = 1 << 28;
    /// Longest time-out (4 ms on display 11+).
    pub const TIME_OUT_MAX: u32 = 3 << 26;
    /// Receive error (write 1 to clear).
    pub const RECEIVE_ERROR: u32 = 1 << 25;
    /// Message size field.
    pub const SIZE_MASK: u32 = genmask(24, 20);
    /// Display 14: AUX power request (kept set across transactions).
    pub const POWER_REQUEST: u32 = 1 << 19;
    /// Display 14: AUX power status.
    pub const POWER_STATUS: u32 = 1 << 18;
    /// Thunderbolt IO.
    pub const TBT_IO: u32 = 1 << 11;
    /// Fast-wake sync pulse count − 1 (9:5).
    pub const FW_SYNC_MASK: u32 = genmask(9, 5);
    /// Sync pulse count − 1 (4:0).
    pub const SYNC_MASK: u32 = genmask(4, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc4_block() {
        assert!(is_tc(6) && !is_tc(1) && !is_tc(7));
        assert_eq!(phy(6), 8);
        assert_eq!(phy(0), 0);
        assert_eq!(buf_ctl1(6), 0x16f800);
        assert_eq!(buf_ctl2(6), 0x16f804);
        assert_eq!(buf_ctl3(6), 0x16f808);
        assert_eq!(aux_ctl(6), 0x16f810);
        assert_eq!(aux_data(6, 4), 0x16f824);
        assert_eq!(msgbus_ctl(6, 1), 0x16f844);
        assert_eq!(msgbus_status(6, 0), 0x16f848);
        assert_eq!(msgbus_timer(6, 0), 0x16f858);
        assert_eq!(clock_ctl(6), 0x16f860);
        assert_eq!(hotplug_ctl(6), 0x16f870);
        assert_eq!(tcss_ddi_status(6), 0x16150c);
        assert_eq!(buf_ctl1(3), 0x16f200);
    }

    #[test]
    fn port_a_block() {
        assert_eq!(buf_ctl1(0), 0x64004);
        assert_eq!(aux_ctl(0), 0x64010);
        assert_eq!(aux_ctl(1), 0x64110);
        assert_eq!(msgbus_ctl(0, 0), 0x64040);
        assert_eq!(msgbus_timer(0, 0), 0x640d8);
        assert_eq!(clock_ctl(1), 0x641e0);
    }

    #[test]
    fn lane_bits() {
        assert_eq!(
            buf2_lane_pipe_reset(0) | buf2_lane_pipe_reset(1),
            0xc000_0000
        );
        assert_eq!(buf2_lane_phy_status(1), 1 << 28);
        assert_eq!(buf2_powerdown_update(0), 1 << 25);
        assert_eq!(buf2_powerdown_state(0, 2), 2 << 20);
        assert_eq!(buf2_powerdown_state(1, 2), 2 << 16);
        assert_eq!(clk_pll_request(0), 1 << 31);
        assert_eq!(clk_pll_ack(1), 1 << 26);
        assert_eq!(clk_refclk_request(1), 1 << 25);
        assert_eq!(clk_refclk_ack(0), 1 << 28);
        assert_eq!(buf1_width_bits(4), 3 << 1);
        assert_eq!(buf1_width_bits(3), 4 << 1);
    }
}
