//! GMBUS: the display engine's I2C controller for DDC on HDMI/DVI connectors (EDID at address
//! 0x50). Sequence as in Linux' `intel_gmbus.c` (MIT): index cycle with the offset, read in 4-byte
//! words, stop cycle; a NAK (no display) ends the transfer cleanly.

use crate::error::{DdcError, Result};
use crate::hw::Hw;

/// `GMBUS0`: rate and pin.
pub const GMBUS0: u32 = 0xc5100;
/// `GMBUS1`: command.
pub const GMBUS1: u32 = 0xc5104;
/// `GMBUS2`: status.
pub const GMBUS2: u32 = 0xc5108;
/// `GMBUS3`: data.
pub const GMBUS3: u32 = 0xc510c;
/// `GMBUS4`: interrupt mask.
pub const GMBUS4: u32 = 0xc5110;

/// GMBUS1: software clear interrupt (resets the controller after a NAK).
pub const SW_CLR_INT: u32 = 1 << 31;
/// GMBUS1: software ready (start).
pub const SW_RDY: u32 = 1 << 30;
/// GMBUS1: cycle "wait".
pub const CYCLE_WAIT: u32 = 1 << 25;
/// GMBUS1: cycle "index" (send an offset first).
pub const CYCLE_INDEX: u32 = 2 << 25;
/// GMBUS1: cycle "stop".
pub const CYCLE_STOP: u32 = 4 << 25;
/// GMBUS1: read.
pub const READ: u32 = 1;
/// GMBUS2: bus in use.
pub const INUSE: u32 = 1 << 15;
/// GMBUS2: hardware in wait phase.
pub const HW_WAIT_PHASE: u32 = 1 << 14;
/// GMBUS2: hardware ready for the next data word.
pub const HW_RDY: u32 = 1 << 11;
/// GMBUS2: slave acknowledge time-out error (NAK).
pub const SATOER: u32 = 1 << 10;
/// GMBUS2: active.
pub const ACTIVE: u32 = 1 << 9;
/// Largest transfer (display 9+).
pub const MAX_BYTES: usize = 511;

fn wait_rdy<H: Hw>(hw: &mut H) -> Result<()> {
    let start = hw.now_us();
    loop {
        let st = hw.read(GMBUS2);
        if st & SATOER != 0 {
            return Err(DdcError::Nack.into());
        }
        if st & HW_RDY != 0 {
            return Ok(());
        }
        if hw.now_us() - start > 50_000 {
            return Err(DdcError::Timeout.into());
        }
        hw.delay_us(10);
    }
}

fn reset<H: Hw>(hw: &mut H) {
    hw.write(GMBUS1, SW_CLR_INT);
    hw.write(GMBUS1, 0);
    hw.write(GMBUS0, 0);
}

/// Reads `buf.len()` bytes from I2C device `dev` (7-bit) at `offset` over GMBUS pin `pin`.
pub fn read<H: Hw>(hw: &mut H, pin: u8, dev: u8, offset: u8, buf: &mut [u8]) -> Result<()> {
    if buf.is_empty() || buf.len() > MAX_BYTES {
        return Err(DdcError::TooLong.into());
    }
    hw.write(GMBUS4, 0);
    hw.write(GMBUS0, pin as u32 & 0x1f); // 100 kHz
    let cmd = SW_RDY
        | CYCLE_INDEX
        | CYCLE_WAIT
        | ((buf.len() as u32) << 16)
        | ((offset as u32) << 8)
        | ((dev as u32) << 1)
        | READ;
    hw.write(GMBUS1, cmd);
    let result = (|| {
        for chunk in buf.chunks_mut(4) {
            wait_rdy(hw)?;
            let w = hw.read(GMBUS3);
            for (i, b) in chunk.iter_mut().enumerate() {
                *b = (w >> (8 * i)) as u8;
            }
        }
        hw.wait_set(GMBUS2, HW_WAIT_PHASE, 50_000)
            .map_err(|_| DdcError::Timeout)?;
        hw.write(GMBUS1, SW_RDY | CYCLE_STOP);
        hw.wait_clear(GMBUS2, ACTIVE, 10_000)
            .map_err(|_| DdcError::Timeout)?;
        hw.write(GMBUS0, 0);
        Ok(())
    })();
    if result.is_err() {
        reset(hw);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dp::sink::test_edid;
    use crate::error::Error;
    use crate::sim::Sim;
    use alloc::boxed::Box;

    #[test]
    fn reads_one_word_and_stops() {
        let mut s = Sim::new();
        let e = test_edid();
        s.set(GMBUS3, u32::from_le_bytes([e[0], e[1], e[2], e[3]]));
        s.on_write(
            GMBUS1,
            Box::new(|regs, _, v| {
                if v & CYCLE_STOP != 0 {
                    regs.insert(GMBUS2, 0);
                } else if v & SW_RDY != 0 {
                    regs.insert(GMBUS2, HW_RDY | HW_WAIT_PHASE | ACTIVE);
                }
            }),
        );
        let mut b = [0u8; 4];
        read(&mut s, 6, 0x50, 0, &mut b).unwrap();
        assert_eq!(b, [0, 0xff, 0xff, 0xff]);
        assert_eq!(s.get(GMBUS0), 0);
        let cmd = s.writes_to(GMBUS1)[0];
        assert_eq!(cmd & 0x1ff_0000, 4 << 16);
        assert_eq!((cmd >> 1) & 0x7f, 0x50);
        assert_eq!(s.writes_to(GMBUS0)[0], 6);
    }

    #[test]
    fn nak_resets_the_controller() {
        let mut s = Sim::new();
        s.on_write(
            GMBUS1,
            Box::new(|regs, _, v| {
                if v & SW_RDY != 0 {
                    regs.insert(GMBUS2, SATOER);
                }
            }),
        );
        let mut b = [0u8; 8];
        assert_eq!(
            read(&mut s, 5, 0x50, 0, &mut b),
            Err(Error::Ddc(DdcError::Nack))
        );
        assert!(s.writes_to(GMBUS1).contains(&SW_CLR_INT));
        assert_eq!(s.get(GMBUS0), 0);
    }

    #[test]
    fn silence_times_out_and_limits_hold() {
        let mut s = Sim::new();
        let mut b = [0u8; 4];
        assert_eq!(
            read(&mut s, 5, 0x50, 0, &mut b),
            Err(Error::Ddc(DdcError::Timeout))
        );
        assert_eq!(
            read(&mut s, 5, 0x50, 0, &mut []),
            Err(Error::Ddc(DdcError::TooLong))
        );
        let mut big = [0u8; 600];
        assert_eq!(
            read(&mut s, 5, 0x50, 0, &mut big),
            Err(Error::Ddc(DdcError::TooLong))
        );
    }
}
