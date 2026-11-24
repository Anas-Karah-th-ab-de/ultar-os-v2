//! CX0 PHY access of display 14 (C10 on ports A/B, C20 on the Type-C ports) over the port's
//! message bus, and the C20 PLL enable sequence for DisplayPort.
//!
//! The order of steps and the timeouts follow Linux' `intel_cx0_phy.c` (MIT): message-bus
//! read/write with acknowledge and bus reset after every transaction (a hardware workaround below
//! display 30), SRAM context writes into the inactive context and a context toggle, lane reset,
//! power-state changes, PCLK PLL request/acknowledge.

use super::c20::{self, C20Pll};
use crate::error::{Error, Result};
use crate::hw::{field, Hw};
use crate::regs::port;

/// Both PHY lanes (each carries two of the four DP lanes).
pub const BOTH_LANES: u8 = 0b11;

fn lanes(mask: u8) -> impl Iterator<Item = u8> {
    (0..2u8).filter(move |l| mask & (1 << l) != 0)
}

fn bus_reset<H: Hw>(hw: &mut H, p: u8, lane: u8) {
    hw.write(port::msgbus_ctl(p, lane), port::M2P_RESET);
    let _ = hw.wait_clear(port::msgbus_ctl(p, lane), port::M2P_RESET, 1000);
    hw.rmw(
        port::msgbus_status(p, lane),
        0,
        port::P2M_RESPONSE_READY | port::P2M_ERROR,
    );
}

fn wait_ack<H: Hw>(hw: &mut H, p: u8, lane: u8, cmd: u32, addr: u16) -> Result<u32> {
    let st = match hw.wait_set(port::msgbus_status(p, lane), port::P2M_RESPONSE_READY, 1000) {
        Ok(v) => v,
        Err(_) => {
            let status = hw.read(port::msgbus_status(p, lane));
            bus_reset(hw, p, lane);
            return Err(Error::MsgBus { addr, status });
        }
    };
    if st & port::P2M_ERROR != 0 || field(st, port::P2M_COMMAND_MASK) != cmd {
        bus_reset(hw, p, lane);
        return Err(Error::MsgBus { addr, status: st });
    }
    Ok(st)
}

fn idle<H: Hw>(hw: &mut H, p: u8, lane: u8, addr: u16) -> Result<()> {
    if let Err(Error::Timeout { last, .. }) =
        hw.wait_clear(port::msgbus_ctl(p, lane), port::M2P_PENDING, 1000)
    {
        bus_reset(hw, p, lane);
        return Err(Error::MsgBus { addr, status: last });
    }
    Ok(())
}

fn read_once<H: Hw>(hw: &mut H, p: u8, lane: u8, addr: u16) -> Result<u8> {
    idle(hw, p, lane, addr)?;
    hw.rmw(
        port::msgbus_status(p, lane),
        0,
        port::P2M_RESPONSE_READY | port::P2M_ERROR,
    );
    hw.write(
        port::msgbus_ctl(p, lane),
        port::M2P_PENDING | port::M2P_READ | (addr as u32 & 0xfff),
    );
    let st = wait_ack(hw, p, lane, port::P2M_READ_ACK, addr)?;
    hw.rmw(
        port::msgbus_status(p, lane),
        0,
        port::P2M_RESPONSE_READY | port::P2M_ERROR,
    );
    bus_reset(hw, p, lane);
    Ok(field(st, port::P2M_DATA_MASK) as u8)
}

fn write_once<H: Hw>(
    hw: &mut H,
    p: u8,
    lane: u8,
    addr: u16,
    data: u8,
    committed: bool,
) -> Result<()> {
    idle(hw, p, lane, addr)?;
    hw.rmw(
        port::msgbus_status(p, lane),
        0,
        port::P2M_RESPONSE_READY | port::P2M_ERROR,
    );
    let cmd = if committed {
        port::M2P_WRITE_COMMITTED
    } else {
        port::M2P_WRITE_UNCOMMITTED
    };
    hw.write(
        port::msgbus_ctl(p, lane),
        port::M2P_PENDING | cmd | (data as u32) << 16 | (addr as u32 & 0xfff),
    );
    idle(hw, p, lane, addr)?;
    if committed {
        wait_ack(hw, p, lane, port::P2M_WRITE_ACK, addr)?;
    } else if hw.read(port::msgbus_status(p, lane)) & port::P2M_ERROR != 0 {
        let status = hw.read(port::msgbus_status(p, lane));
        bus_reset(hw, p, lane);
        return Err(Error::MsgBus { addr, status });
    }
    hw.rmw(
        port::msgbus_status(p, lane),
        0,
        port::P2M_RESPONSE_READY | port::P2M_ERROR,
    );
    bus_reset(hw, p, lane);
    Ok(())
}

/// Reads PHY register `addr` of PHY lane `lane` (three tries).
pub fn read<H: Hw>(hw: &mut H, p: u8, lane: u8, addr: u16) -> Result<u8> {
    let mut last = Err(Error::MsgBus { addr, status: 0 });
    for _ in 0..3 {
        last = read_once(hw, p, lane, addr);
        if last.is_ok() {
            break;
        }
    }
    last
}

/// Writes PHY register `addr` on every lane of `mask` (three tries each).
pub fn write<H: Hw>(
    hw: &mut H,
    p: u8,
    mask: u8,
    addr: u16,
    data: u8,
    committed: bool,
) -> Result<()> {
    for lane in lanes(mask) {
        let mut last = Err(Error::MsgBus { addr, status: 0 });
        for _ in 0..3 {
            last = write_once(hw, p, lane, addr, data, committed);
            if last.is_ok() {
                break;
            }
        }
        last?;
    }
    Ok(())
}

/// Read-modify-write on every lane of `mask`; writes only when the value changes.
pub fn rmw<H: Hw>(hw: &mut H, p: u8, mask: u8, addr: u16, clear: u8, set: u8) -> Result<()> {
    for lane in lanes(mask) {
        let old = read(hw, p, lane, addr)?;
        let new = (old & !clear) | set;
        if new != old {
            write(hw, p, 1 << lane, addr, new, true)?;
        }
    }
    Ok(())
}

/// Writes a 16-bit word into the C20 SRAM through lane 0.
pub fn sram_write<H: Hw>(hw: &mut H, p: u8, addr: u16, data: u16) -> Result<()> {
    write(hw, p, 1, c20::sram::WR_ADDR_H, (addr >> 8) as u8, false)?;
    write(hw, p, 1, c20::sram::WR_ADDR_L, addr as u8, false)?;
    write(hw, p, 1, c20::sram::WR_DATA_H, (data >> 8) as u8, false)?;
    write(hw, p, 1, c20::sram::WR_DATA_L, data as u8, true)
}

/// Reads a 16-bit word from the C20 SRAM through lane 0.
pub fn sram_read<H: Hw>(hw: &mut H, p: u8, addr: u16) -> Result<u16> {
    write(hw, p, 1, c20::sram::RD_ADDR_H, (addr >> 8) as u8, false)?;
    write(hw, p, 1, c20::sram::RD_ADDR_L, addr as u8, true)?;
    let hi = read(hw, p, 0, c20::sram::RD_DATA_H)? as u16;
    let lo = read(hw, p, 0, c20::sram::RD_DATA_L)? as u16;
    Ok(hi << 8 | lo)
}

/// Programs the message-bus timer of both lanes (Intel asks for 0xA000 before any transaction).
pub fn program_timer<H: Hw>(hw: &mut H, p: u8) {
    for lane in 0..2 {
        hw.rmw(
            port::msgbus_timer(p, lane),
            port::MSGBUS_TIMER_MASK,
            port::MSGBUS_TIMER_VAL,
        );
    }
}

/// Writes a C20 configuration into the context that is not in use, sets the DP vendor registers
/// and toggles the context so the PHY loads it.
pub fn c20_program<H: Hw>(hw: &mut H, p: u8, owned: u8, pll: &C20Pll, rate: u32) -> Result<()> {
    let serdes = c20::serdes_dp(rate).ok_or(Error::Invalid("link rate"))?;
    let cur = read(hw, p, 0, c20::VDR_SERDES_RATE)? & c20::CONTEXT_TOGGLE != 0;
    // context A in use -> write B, and the other way round (as Linux: "if A in use, write B")
    let b = !cur;
    for (i, w) in pll.tx.iter().enumerate() {
        sram_write(hw, p, c20::tx_cfg(b, i), *w)?;
    }
    for (i, w) in pll.cmn.iter().enumerate() {
        sram_write(hw, p, c20::cmn_cfg(b, i), *w)?;
    }
    for (i, w) in pll.mpllb.iter().enumerate() {
        sram_write(hw, p, c20::mpllb_cfg(b, i), *w)?;
    }
    rmw(hw, p, owned, c20::VDR_CUSTOM_WIDTH, 0b11, 0)?;
    rmw(hw, p, owned, c20::VDR_SERDES_RATE, c20::SERDES_MASK, serdes)?;
    rmw(
        hw,
        p,
        owned,
        c20::VDR_SERDES_RATE,
        c20::CONTEXT_TOGGLE,
        if cur { 0 } else { c20::CONTEXT_TOGGLE },
    )
}
