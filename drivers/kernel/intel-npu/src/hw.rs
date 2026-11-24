//! Register access as the sequences of this crate need it. The kernel implements [`Hw`] over the
//! two mapped BARs (volatile 32-bit accesses, a busy-wait delay and the monotonic clock); the
//! tests use the simulated register file in `sim`.

use crate::error::{Error, Result};
use core::fmt;

/// Which register block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Space {
    /// The NPU's own subsystems (BAR0). Only readable while the NPU is powered.
    Ip,
    /// Power, clock and reset control (BAR4).
    Buttress,
}

/// A register: block and byte offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reg(pub Space, pub u32);

impl Reg {
    /// A register of the IP block.
    pub const fn ip(off: u32) -> Reg {
        Reg(Space::Ip, off)
    }
    /// A register of the buttress.
    pub const fn b(off: u32) -> Reg {
        Reg(Space::Buttress, off)
    }
}

impl fmt::Display for Reg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let block = match self.0 {
            Space::Ip => "ip",
            Space::Buttress => "buttress",
        };
        write!(f, "{block}+{:#x}", self.1)
    }
}

/// 32-bit register access to the NPU plus time.
pub trait Hw {
    /// Reads a register.
    fn read(&mut self, reg: Reg) -> u32;
    /// Writes a register.
    fn write(&mut self, reg: Reg, val: u32);
    /// Waits at least `us` microseconds.
    fn delay_us(&mut self, us: u32);
    /// Monotonic time in microseconds.
    fn now_us(&mut self) -> u64;

    /// Read-modify-write: clears `clear`, sets `set`.
    fn rmw(&mut self, reg: Reg, clear: u32, set: u32) {
        let old = self.read(reg);
        self.write(reg, (old & !clear) | set);
    }

    /// Waits until `read(reg) & mask == value`, at most `timeout_us` (polled every 50 µs). A
    /// register that reads all ones is not accepted (an unpowered block reads so and may still
    /// come up); if it still does at the time limit the device counts as gone.
    fn poll(
        &mut self,
        what: &'static str,
        reg: Reg,
        mask: u32,
        value: u32,
        timeout_us: u32,
    ) -> Result<u32> {
        let start = self.now_us();
        loop {
            let v = self.read(reg);
            if v != u32::MAX && v & mask == value {
                return Ok(v);
            }
            if self.now_us().saturating_sub(start) >= timeout_us as u64 {
                return Err(if v == u32::MAX {
                    Error::DeviceGone
                } else {
                    Error::Timeout { what, reg, last: v }
                });
            }
            self.delay_us(50.min(timeout_us.max(1)));
        }
    }

    /// Reads once and requires `read(reg) & mask == value`.
    fn check(&mut self, what: &'static str, reg: Reg, mask: u32, value: u32) -> Result<()> {
        let val = self.read(reg);
        if val == u32::MAX {
            return Err(Error::DeviceGone);
        }
        if val & mask == value {
            Ok(())
        } else {
            Err(Error::Check { what, reg, val })
        }
    }
}

/// Extracts the field `mask` (contiguous bits) of `v`, shifted down.
pub const fn field(v: u32, mask: u32) -> u32 {
    (v & mask) >> mask.trailing_zeros()
}

/// Places `val` into the field `mask` (contiguous bits).
pub const fn prep(mask: u32, val: u32) -> u32 {
    (val << mask.trailing_zeros()) & mask
}

/// Mask of bits `hi..=lo`.
pub const fn genmask(hi: u32, lo: u32) -> u32 {
    (u32::MAX >> (31 - hi)) & (u32::MAX << lo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;

    #[test]
    fn field_helpers() {
        assert_eq!(genmask(15, 8), 0xff00);
        assert_eq!(field(0x1234, genmask(15, 8)), 0x12);
        assert_eq!(prep(genmask(31, 16), 0x80), 0x80_0000);
    }

    #[test]
    fn poll_and_check() {
        let mut s = Sim::new();
        let r = Reg::b(0x40);
        s.after_reads(r, 3, 0x2);
        assert_eq!(s.poll("lock", r, 0x2, 0x2, 1000), Ok(0x2));
        assert_eq!(
            s.poll("unlock", r, 0x2, 0, 200),
            Err(Error::Timeout {
                what: "unlock",
                reg: r,
                last: 0x2
            })
        );
        assert!(s.check("x", r, 0x2, 0x2).is_ok());
        assert_eq!(
            s.check("x", r, 0x2, 0),
            Err(Error::Check {
                what: "x",
                reg: r,
                val: 2
            })
        );
        s.set(r, u32::MAX);
        assert_eq!(s.poll("lock", r, 0x2, 0x2, 10), Err(Error::DeviceGone));
        assert_eq!(s.check("x", r, 0x2, 0x2), Err(Error::DeviceGone));
        // all ones first (block not powered yet), then the real value
        s.after_reads(r, 2, 0x2);
        assert_eq!(s.poll("lock", r, 0x2, 0x2, 1000), Ok(0x2));
    }
}
