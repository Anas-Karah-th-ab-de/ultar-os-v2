//! Pipe CRC: the hardware computes a checksum of every frame it sends. The same picture gives the
//! same value frame after frame; a different picture a different value. The self test uses it to
//! prove that the scanned-out content is the buffer the driver wrote.

use crate::error::{Error, Result};
use crate::hw::Hw;
use crate::regs::{self, irq, trans};
use alloc::vec::Vec;

/// `PIPE_CRC_CTL`: enable.
pub const CRC_ENABLE: u32 = 1 << 31;
/// Source: the pipe output after blending (display 9+ "DMUX").
pub const SOURCE_PIPE: u32 = 4 << 28;
/// Source: plane 1 only.
pub const SOURCE_PLANE1: u32 = 0;

/// Starts CRC generation on transcoder/pipe `p` and clears stale results.
pub fn start<H: Hw>(hw: &mut H, p: u8, source: u32) {
    hw.write(trans(p, regs::CRC_CTL), 0);
    hw.write(irq::pipe_iir(p), irq::PIPE_CRC_DONE);
    hw.write(trans(p, regs::CRC_CTL), CRC_ENABLE | source);
}

/// Stops CRC generation.
pub fn stop<H: Hw>(hw: &mut H, p: u8) {
    hw.write(trans(p, regs::CRC_CTL), 0);
    hw.write(irq::pipe_iir(p), irq::PIPE_CRC_DONE);
}

/// Waits for the next frame's CRC (CRC-done latched in the pipe's identity register).
pub fn next<H: Hw>(hw: &mut H, p: u8, timeout_us: u32) -> Result<u32> {
    hw.wait_set(irq::pipe_iir(p), irq::PIPE_CRC_DONE, timeout_us)?;
    let v = hw.read(trans(p, regs::CRC_RES));
    hw.write(irq::pipe_iir(p), irq::PIPE_CRC_DONE);
    Ok(v)
}

/// Collects `n` frame CRCs after skipping `skip` (the first ones after a change may mix frames).
pub fn collect<H: Hw>(
    hw: &mut H,
    p: u8,
    skip: usize,
    n: usize,
    frame_timeout_us: u32,
) -> Result<Vec<u32>> {
    let mut out = Vec::with_capacity(n);
    for i in 0..skip + n {
        let v = next(hw, p, frame_timeout_us)?;
        if i >= skip {
            out.push(v);
        }
    }
    Ok(out)
}

/// The value if all CRCs are equal (a still picture), else `None`.
pub fn stable(crcs: &[u32]) -> Option<u32> {
    let first = *crcs.first()?;
    crcs.iter().all(|c| *c == first).then_some(first)
}

/// Verdict of a CRC comparison of two pictures A and B shown in the order A, B, A.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// CRC of picture A (first time).
    pub a: u32,
    /// CRC of picture B.
    pub b: u32,
    /// CRC of picture A (second time).
    pub a_again: u32,
}

impl Verdict {
    /// From the three stable CRCs; `Err` if any picture did not give a stable value.
    pub fn from_runs(a: &[u32], b: &[u32], a_again: &[u32]) -> Result<Verdict> {
        let s = |v: &[u32]| stable(v).ok_or(Error::Invalid("CRC not stable"));
        Ok(Verdict {
            a: s(a)?,
            b: s(b)?,
            a_again: s(a_again)?,
        })
    }

    /// A and B differ and A gives the same CRC again: the hardware shows what the driver wrote.
    pub fn ok(&self) -> bool {
        self.a == self.a_again && self.a != self.b && self.a != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;

    /// Simulates a pipe that produces `crc` every time the done bit is cleared.
    fn pipe_with_crc(s: &mut Sim, crc: u32) {
        s.set(0x60064, crc);
        s.set(0x44408, irq::PIPE_CRC_DONE);
        s.on_write(
            0x44408,
            alloc::boxed::Box::new(|regs, reg, v| {
                // write-1-to-clear, then the next frame sets it again
                let _ = v;
                regs.insert(reg, irq::PIPE_CRC_DONE);
            }),
        );
    }

    #[test]
    fn start_collect_stop() {
        let mut s = Sim::new();
        pipe_with_crc(&mut s, 0xdead_beef);
        start(&mut s, 0, SOURCE_PIPE);
        assert_eq!(s.get(0x60050), CRC_ENABLE | SOURCE_PIPE);
        let v = collect(&mut s, 0, 2, 3, 20_000).unwrap();
        assert_eq!(v, alloc::vec![0xdead_beef; 3]);
        assert_eq!(stable(&v), Some(0xdead_beef));
        stop(&mut s, 0);
        assert_eq!(s.get(0x60050), 0);
    }

    #[test]
    fn missing_frames_time_out() {
        let mut s = Sim::new();
        s.on_write(
            irq::pipe_iir(1),
            alloc::boxed::Box::new(|regs, reg, _| {
                regs.insert(reg, 0); // write 1 to clear
            }),
        );
        start(&mut s, 1, SOURCE_PLANE1);
        assert!(matches!(next(&mut s, 1, 1000), Err(Error::Timeout { .. })));
    }

    #[test]
    fn verdicts() {
        assert_eq!(stable(&[]), None);
        assert_eq!(stable(&[1, 2]), None);
        let v = Verdict::from_runs(&[5, 5], &[7, 7], &[5]).unwrap();
        assert!(v.ok());
        assert!(!Verdict {
            a: 5,
            b: 5,
            a_again: 5
        }
        .ok());
        assert!(!Verdict {
            a: 5,
            b: 6,
            a_again: 7
        }
        .ok());
        assert!(!Verdict {
            a: 0,
            b: 6,
            a_again: 0
        }
        .ok());
        assert_eq!(
            Verdict::from_runs(&[1, 2], &[3], &[1]),
            Err(Error::Invalid("CRC not stable"))
        );
    }
}
