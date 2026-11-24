//! A simulated CX0/C20 PHY behind the message bus of one port, plus the port registers that
//! answer the PLL enable sequence (SoC ready, lane reset status, power-down updates, clock acks).

use crate::hw::field;
use crate::regs::port;
use crate::sim::Sim;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use core::cell::RefCell;

/// PHY state visible to the tests.
#[derive(Default)]
pub struct Phy {
    /// PHY registers per lane: (lane, address) → value.
    pub regs: BTreeMap<(u8, u16), u8>,
    /// C20 SRAM.
    pub sram: BTreeMap<u16, u16>,
    /// Fail every transaction with an error status.
    pub broken: bool,
    wr_addr: u16,
    wr_hi: u8,
    rd_addr: u16,
}

impl Phy {
    fn access(&mut self, lane: u8, cmd: u32, addr: u16, data: u8) -> u8 {
        use super::c20::sram;
        match cmd {
            port::M2P_READ => match addr {
                sram::RD_DATA_H => (self.sram.get(&self.rd_addr).copied().unwrap_or(0) >> 8) as u8,
                sram::RD_DATA_L => self.sram.get(&self.rd_addr).copied().unwrap_or(0) as u8,
                _ => self.regs.get(&(lane, addr)).copied().unwrap_or(0),
            },
            _ => {
                match addr {
                    sram::WR_ADDR_H => self.wr_addr = (self.wr_addr & 0xff) | (data as u16) << 8,
                    sram::WR_ADDR_L => self.wr_addr = (self.wr_addr & 0xff00) | data as u16,
                    sram::WR_DATA_H => self.wr_hi = data,
                    sram::WR_DATA_L => {
                        self.sram
                            .insert(self.wr_addr, (self.wr_hi as u16) << 8 | data as u16);
                    }
                    sram::RD_ADDR_H => self.rd_addr = (self.rd_addr & 0xff) | (data as u16) << 8,
                    sram::RD_ADDR_L => self.rd_addr = (self.rd_addr & 0xff00) | data as u16,
                    _ => {
                        self.regs.insert((lane, addr), data);
                    }
                }
                0
            }
        }
    }
}

/// Attaches a PHY to port `p` of `sim`.
pub fn attach(sim: &mut Sim, p: u8) -> Rc<RefCell<Phy>> {
    let phy = Rc::new(RefCell::new(Phy::default()));
    for lane in 0..2u8 {
        let ph = phy.clone();
        sim.on_write(
            port::msgbus_ctl(p, lane),
            Box::new(move |regs, reg, v| {
                if v & port::M2P_RESET != 0 || v & port::M2P_PENDING == 0 {
                    regs.insert(reg, 0);
                    return;
                }
                let cmd = v & (0xf << 27);
                let addr = (v & 0xfff) as u16;
                let data = field(v, port::P2M_DATA_MASK) as u8;
                let mut phy = ph.borrow_mut();
                let out = phy.access(lane, cmd, addr, data);
                regs.insert(reg, v & !port::M2P_PENDING);
                let st = port::msgbus_status(p, lane);
                if phy.broken {
                    regs.insert(st, port::P2M_RESPONSE_READY | port::P2M_ERROR);
                } else if cmd == port::M2P_READ {
                    regs.insert(
                        st,
                        port::P2M_RESPONSE_READY | port::P2M_READ_ACK << 27 | (out as u32) << 16,
                    );
                } else if cmd == port::M2P_WRITE_COMMITTED {
                    regs.insert(st, port::P2M_RESPONSE_READY | port::P2M_WRITE_ACK << 27);
                }
            }),
        );
        sim.on_write(
            port::msgbus_status(p, lane),
            Box::new(|regs, reg, v| {
                // write 1 to clear
                regs.insert(reg, 0);
                let _ = v;
            }),
        );
    }
    port_regs(sim, p);
    phy
}

/// SoC ready, lane reset status, power-down updates and clock acknowledges of port `p`.
fn port_regs(sim: &mut Sim, p: u8) {
    sim.set(port::buf_ctl1(p), port::BUF1_SOC_PHY_READY);
    sim.on_write(
        port::buf_ctl2(p),
        Box::new(|regs, reg, v| {
            let mut n = v & !(port::buf2_powerdown_update(0) | port::buf2_powerdown_update(1));
            let resets = port::buf2_lane_pipe_reset(0) | port::buf2_lane_pipe_reset(1);
            let status = port::buf2_lane_phy_status(0) | port::buf2_lane_phy_status(1);
            n = if v & resets != 0 {
                n | status
            } else {
                n & !status
            };
            regs.insert(reg, n);
        }),
    );
    sim.on_write(
        port::clock_ctl(p),
        Box::new(|regs, reg, v| {
            let mut n = v & !(port::clk_pll_ack(0)
                | port::clk_pll_ack(1)
                | port::clk_refclk_ack(0)
                | port::clk_refclk_ack(1));
            for l in 0..2 {
                if v & port::clk_pll_request(l) != 0 {
                    n |= port::clk_pll_ack(l);
                }
                if v & port::clk_refclk_request(l) != 0 {
                    n |= port::clk_refclk_ack(l);
                }
            }
            regs.insert(reg, n);
        }),
    );
}
