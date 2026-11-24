//! Register access as the sequences of this crate need it. The kernel implements [`Hw`] over the
//! mapped BAR0 (volatile 32-bit accesses, a busy-wait delay and the monotonic clock); the tests use
//! the simulated register file in `sim`.

use crate::error::{Error, Result};

/// 32-bit register access to the display engine plus time.
pub trait Hw {
    /// Reads the register at `reg` (byte offset in BAR0).
    fn read(&mut self, reg: u32) -> u32;
    /// Writes `val` to the register at `reg`.
    fn write(&mut self, reg: u32, val: u32);
    /// Writes half of a GGTT entry (`reg` in the upper half of BAR0). Only `ggtt` calls this; the
    /// kernel's `write` refuses that half, so no computed register offset can reach the table.
    fn write_gtt(&mut self, reg: u32, val: u32) {
        self.write(reg, val);
    }
    /// Waits at least `us` microseconds (busy wait; the callers keep waits short).
    fn delay_us(&mut self, us: u32);
    /// Monotonic time in microseconds.
    fn now_us(&mut self) -> u64;

    /// Read-modify-write: clears `clear`, sets `set`, returns the old value.
    fn rmw(&mut self, reg: u32, clear: u32, set: u32) -> u32 {
        let old = self.read(reg);
        self.write(reg, (old & !clear) | set);
        old
    }

    /// Waits until `read(reg) & mask == value`, at most `timeout_us`; returns the last value.
    fn wait(&mut self, reg: u32, mask: u32, value: u32, timeout_us: u32) -> Result<u32> {
        let start = self.now_us();
        loop {
            let v = self.read(reg);
            if v & mask == value {
                return Ok(v);
            }
            if self.now_us().saturating_sub(start) >= timeout_us as u64 {
                return Err(Error::Timeout { reg, last: v });
            }
            self.delay_us(if timeout_us > 2000 { 10 } else { 1 });
        }
    }

    /// Waits until all bits of `mask` are set.
    fn wait_set(&mut self, reg: u32, mask: u32, timeout_us: u32) -> Result<u32> {
        self.wait(reg, mask, mask, timeout_us)
    }

    /// Waits until all bits of `mask` are clear.
    fn wait_clear(&mut self, reg: u32, mask: u32, timeout_us: u32) -> Result<u32> {
        self.wait(reg, mask, 0, timeout_us)
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
        assert_eq!(genmask(3, 1), 0b1110);
        assert_eq!(genmask(31, 0), u32::MAX);
        assert_eq!(genmask(31, 31), 1 << 31);
        assert_eq!(field(0xba03_0006, genmask(3, 1)), 3);
        assert_eq!(prep(genmask(27, 24), 4), 0x0400_0000);
        assert_eq!(prep(genmask(3, 0), 0x1f), 0xf);
    }

    #[test]
    fn rmw_and_wait() {
        let mut s = Sim::new();
        s.set(0x100, 0xf0);
        assert_eq!(s.rmw(0x100, 0x30, 0x1), 0xf0);
        assert_eq!(s.get(0x100), 0xc1);
        assert_eq!(s.wait_set(0x100, 0xc0, 10), Ok(0xc1));
        assert_eq!(
            s.wait_clear(0x100, 0x1, 50),
            Err(Error::Timeout {
                reg: 0x100,
                last: 0xc1
            })
        );
        assert!(s.now_us() >= 50);
    }

    #[test]
    fn wait_sees_a_later_change() {
        let mut s = Sim::new();
        s.after_reads(0x200, 3, 0x8);
        assert_eq!(s.wait_set(0x200, 0x8, 3000), Ok(0x8));
    }
}
