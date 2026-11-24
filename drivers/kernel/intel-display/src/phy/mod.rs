//! The PHYs of display 14: C20 tables, message-bus access, PLL enable/disable and drive levels.

pub mod c20;
pub mod cx0;
pub mod pll;

#[cfg(test)]
pub(crate) mod sim_phy;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::hw::Hw;
    use crate::regs::{ddi, port};
    use crate::sim::Sim;

    #[test]
    fn msgbus_read_write_rmw() {
        let mut s = Sim::new();
        let phy = sim_phy::attach(&mut s, 6);
        cx0::write(&mut s, 6, cx0::BOTH_LANES, 0xd02, 0x5, true).unwrap();
        assert_eq!(phy.borrow().regs.get(&(0, 0xd02)), Some(&5));
        assert_eq!(phy.borrow().regs.get(&(1, 0xd02)), Some(&5));
        assert_eq!(cx0::read(&mut s, 6, 1, 0xd02).unwrap(), 5);
        cx0::rmw(&mut s, 6, 1, 0xd02, 0x1, 0x2).unwrap();
        assert_eq!(cx0::read(&mut s, 6, 0, 0xd02).unwrap(), 6);
        assert_eq!(cx0::read(&mut s, 6, 1, 0xd02).unwrap(), 5);
        // the bus is reset after every transaction
        assert!(s
            .writes_to(port::msgbus_ctl(6, 0))
            .contains(&port::M2P_RESET));
    }

    #[test]
    fn sram_round_trip() {
        let mut s = Sim::new();
        let phy = sim_phy::attach(&mut s, 6);
        cx0::sram_write(&mut s, 6, 0xcf2e, 0xbe88).unwrap();
        assert_eq!(phy.borrow().sram.get(&0xcf2e), Some(&0xbe88));
        assert_eq!(cx0::sram_read(&mut s, 6, 0xcf2e).unwrap(), 0xbe88);
    }

    #[test]
    fn errors_and_silence_are_reported() {
        let mut s = Sim::new();
        let phy = sim_phy::attach(&mut s, 6);
        phy.borrow_mut().broken = true;
        assert!(matches!(
            cx0::read(&mut s, 6, 0, 0xd00),
            Err(Error::MsgBus { addr: 0xd00, .. })
        ));
        let mut quiet = Sim::new();
        assert!(matches!(
            cx0::write(&mut quiet, 5, 1, 0x10, 1, true),
            Err(Error::MsgBus { addr: 0x10, .. })
        ));
    }

    #[test]
    fn c20_program_writes_the_inactive_context_and_toggles() {
        let mut s = Sim::new();
        let phy = sim_phy::attach(&mut s, 6);
        cx0::c20_program(&mut s, 6, cx0::BOTH_LANES, &c20::DP_RBR, 162_000).unwrap();
        let p = phy.borrow();
        // context A was active (toggle 0) -> B written, toggle set
        assert_eq!(p.sram.get(&c20::tx_cfg(true, 0)), Some(&0xbe88));
        assert_eq!(p.sram.get(&c20::mpllb_cfg(true, 0)), Some(&0x50a8));
        assert_eq!(p.sram.get(&c20::cmn_cfg(true, 1)), Some(&0x0005));
        assert_eq!(p.regs.get(&(0, c20::VDR_SERDES_RATE)), Some(&(0x40 | 1)));
        assert_eq!(p.regs.get(&(1, c20::VDR_SERDES_RATE)), Some(&(0x40 | 1)));
        drop(p);
        cx0::c20_program(&mut s, 6, cx0::BOTH_LANES, &c20::DP_HBR1, 270_000).unwrap();
        let p = phy.borrow();
        assert_eq!(p.sram.get(&c20::mpllb_cfg(false, 0)), Some(&0x308c));
        assert_eq!(p.regs.get(&(0, c20::VDR_SERDES_RATE)), Some(&0x42));
        drop(p);
        assert_eq!(pll::read_dp_rate(&mut s, 6), Ok(Some(270_000)));
        assert_eq!(
            cx0::c20_program(&mut s, 6, 3, &c20::DP_RBR, 1),
            Err(Error::Invalid("link rate"))
        );
    }

    #[test]
    fn pll_enable_sequence() {
        let mut s = Sim::new();
        let phy = sim_phy::attach(&mut s, 6);
        pll::enable_dp(&mut s, 6, cx0::BOTH_LANES, 162_000, 4).unwrap();
        assert!(pll::enabled(&mut s, 6));
        assert_eq!(s.get(ddi::clk_valfreq(6)), 162_000);
        let clk = s.get(port::clock_ctl(6));
        assert_eq!(
            clk & port::CLK_DDI_SELECT_MASK,
            port::CLK_DDI_SELECT_MAXPCLK
        );
        assert!(clk & port::CLK_FORWARD_UNGATE != 0);
        // all four transmitters on
        for lane in 0..2 {
            for tx in 1..=2 {
                assert_eq!(
                    phy.borrow()
                        .regs
                        .get(&(lane, c20::tx_control2(tx)))
                        .copied()
                        .unwrap_or(0)
                        & 0x40,
                    0
                );
            }
        }
        // the lane resets were released
        assert_eq!(
            s.get(port::buf_ctl2(6))
                & (port::buf2_lane_pipe_reset(0) | port::buf2_lane_pipe_reset(1)),
            0
        );
        pll::disable(&mut s, 6).unwrap();
        assert!(!pll::enabled(&mut s, 6));
        assert_eq!(s.get(ddi::clk_valfreq(6)), 0);
    }

    #[test]
    fn two_lanes_disable_the_upper_transmitters() {
        let mut s = Sim::new();
        let phy = sim_phy::attach(&mut s, 6);
        pll::enable_dp(&mut s, 6, cx0::BOTH_LANES, 270_000, 2).unwrap();
        let p = phy.borrow();
        assert_eq!(p.regs.get(&(1, c20::tx_control2(1))), Some(&0x40));
        assert_eq!(p.regs.get(&(1, c20::tx_control2(2))), Some(&0x40));
        assert_eq!(
            p.regs.get(&(0, c20::tx_control2(1))).copied().unwrap_or(0),
            0
        );
    }

    #[test]
    fn pll_without_soc_ready_fails() {
        let mut s = Sim::new();
        let _phy = sim_phy::attach(&mut s, 6);
        s.set(port::buf_ctl1(6), 0);
        assert!(matches!(
            pll::enable_dp(&mut s, 6, 3, 162_000, 4),
            Err(Error::Timeout { .. })
        ));
        assert_eq!(
            pll::enable_dp(&mut s, 6, 3, 1, 4),
            Err(Error::Invalid("link rate"))
        );
    }

    #[test]
    fn drive_levels() {
        let mut s = Sim::new();
        let phy = sim_phy::attach(&mut s, 6);
        pll::set_level(&mut s, 6, cx0::BOTH_LANES, 4, (1, 1)).unwrap();
        let p = phy.borrow();
        // preset 5: swing 34, pre 0, post 5 on every lane
        assert_eq!(p.regs.get(&(0, c20::vdrovrd(0, 0, 1))), Some(&34));
        assert_eq!(p.regs.get(&(1, c20::vdrovrd(1, 1, 2))), Some(&5));
        assert_eq!(p.regs.get(&(0, c20::VDR_OVRD)), Some(&0b101));
        drop(p);
        assert_eq!(s.read(port::buf_ctl1(6)), port::BUF1_SOC_PHY_READY);
    }
}
