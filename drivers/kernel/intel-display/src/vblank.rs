//! Vertical blanks from the hardware: the pipe's 32-bit frame counter, extended to 64 bits, and the
//! time stamp the pipe latches at the start of every blank, turned into the kernel's monotonic
//! time. The rate of the display time base differs between generations (measured on the Arc 140T:
//! not the 1 MHz older parts use), so the driver measures it against the CPU clock at takeover
//! ([`ts_khz`]) and converts with that.

use crate::hw::Hw;
use crate::regs::{self, pipe};

/// One blank: count and monotonic time in nanoseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stamp {
    /// Frame count (64-bit, never goes backwards while the pipe runs).
    pub count: u64,
    /// Time of the blank's start (`CLOCK_MONOTONIC` ns).
    pub time_ns: u64,
}

/// Extends the 32-bit hardware frame counter.
#[derive(Clone, Copy, Debug, Default)]
pub struct Counter {
    last: Option<u32>,
    high: u64,
}

impl Counter {
    /// Feeds a raw counter value; returns the 64-bit count.
    pub fn update(&mut self, raw: u32) -> u64 {
        if let Some(last) = self.last {
            if raw < last {
                self.high += 1 << 32;
            }
        }
        self.last = Some(raw);
        self.high | raw as u64
    }

    /// Forgets the history (the pipe was turned off: its counter restarts).
    pub fn reset(&mut self) {
        self.last = None;
    }
}

/// Raw values read together within one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawBlank {
    /// `PIPE_FRMCOUNT`.
    pub frame: u32,
    /// `PIPE_FRMTMSTMP` (µs time base).
    pub frame_ts: u32,
    /// `TIMESTAMP_CTR` at the time of reading (µs time base).
    pub now_ts: u32,
}

/// Reads counter and time stamps of `p` consistently (retries if a blank starts in between).
pub fn read_raw<H: Hw>(hw: &mut H, p: u8) -> RawBlank {
    let mut raw = RawBlank {
        frame: 0,
        frame_ts: 0,
        now_ts: 0,
    };
    for _ in 0..4 {
        let ts0 = hw.read(pipe(p, regs::FRMTMSTMP));
        let frame = hw.read(pipe(p, regs::FRMCOUNT));
        let now_ts = hw.read(regs::TIMESTAMP_CTR);
        let ts1 = hw.read(pipe(p, regs::FRMTMSTMP));
        raw = RawBlank {
            frame,
            frame_ts: ts1,
            now_ts,
        };
        if ts0 == ts1 {
            break;
        }
    }
    raw
}

/// The blank's time: `now_ns` (read right after `raw`) minus the age of the blank; the display
/// time base runs at `ts_khz`.
pub fn stamp(counter: &mut Counter, raw: RawBlank, now_ns: u64, ts_khz: u32) -> Stamp {
    let age_ticks = raw.now_ts.wrapping_sub(raw.frame_ts) as u64;
    let age_ns = age_ticks * 1_000_000 / ts_khz.max(1) as u64;
    Stamp {
        count: counter.update(raw.frame),
        time_ns: now_ns.saturating_sub(age_ns),
    }
}

/// Rate of the display time base from two readings of `TIMESTAMP_CTR` `ns` nanoseconds apart
/// (`None` if it does not run).
pub fn ts_khz(ticks: u32, ns: u64) -> Option<u32> {
    (ticks > 0 && ns > 0).then(|| (ticks as u64 * 1_000_000 / ns).min(u32::MAX as u64) as u32)
}

/// Refresh rate in mHz from `frames` over `ticks` of a time base running at `ts_khz`.
pub fn rate_from_ticks(frames: u64, ticks: u64, ts_khz: u32) -> Option<u64> {
    (frames > 0 && ticks > 0).then(|| frames * ts_khz as u64 * 1_000_000 / ticks)
}

/// Measured refresh rate between two stamps in millihertz (`None` if no frame passed).
pub fn rate_mhz(a: Stamp, b: Stamp) -> Option<u64> {
    let frames = b.count.checked_sub(a.count).filter(|f| *f > 0)?;
    let ns = b.time_ns.checked_sub(a.time_ns).filter(|n| *n > 0)?;
    Some(frames * 1_000_000_000_000 / ns)
}

/// Whether `measured` is within `tolerance_permille` of `expected` (both mHz).
pub fn within(measured: u64, expected: u64, tolerance_permille: u64) -> bool {
    measured.abs_diff(expected) * 1000 <= expected * tolerance_permille
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;

    #[test]
    fn counter_extends_across_wrap() {
        let mut c = Counter::default();
        assert_eq!(c.update(0xffff_fffe), 0xffff_fffe);
        assert_eq!(c.update(0xffff_ffff), 0xffff_ffff);
        assert_eq!(c.update(1), 0x1_0000_0001);
        assert_eq!(c.update(1), 0x1_0000_0001);
        c.reset();
        assert_eq!(c.update(0), 0x1_0000_0000);
    }

    #[test]
    fn stamps_from_hardware_time_base() {
        let mut s = Sim::new();
        s.set(0x70040, 1000);
        s.set(0x70048, 5_000_000);
        s.set(0x44070, 5_004_000);
        let raw = read_raw(&mut s, 0);
        assert_eq!(
            raw,
            RawBlank {
                frame: 1000,
                frame_ts: 5_000_000,
                now_ts: 5_004_000
            }
        );
        let mut c = Counter::default();
        let st = stamp(&mut c, raw, 10_000_000_000, 1000);
        assert_eq!(
            st,
            Stamp {
                count: 1000,
                time_ns: 9_996_000_000
            }
        );
        // time base wrapped between blank and read
        let w = RawBlank {
            frame: 1,
            frame_ts: u32::MAX - 9,
            now_ts: 10,
        };
        assert_eq!(
            stamp(&mut c, w, 1_000_000, 1000).time_ns,
            1_000_000 - 20_000
        );
        // a 19.2 MHz time base: 19 200 ticks are 1 ms
        let f = RawBlank {
            frame: 2,
            frame_ts: 0,
            now_ts: 19_200,
        };
        assert_eq!(stamp(&mut c, f, 5_000_000, 19_200).time_ns, 4_000_000);
    }

    #[test]
    fn read_raw_retries_when_a_blank_starts() {
        let mut s = Sim::new();
        s.set(0x70048, 100);
        s.after_reads(0x70048, 1, 200); // second read sees the new blank
        s.set(0x70040, 7);
        let raw = read_raw(&mut s, 0);
        assert_eq!(raw.frame_ts, 200);
    }

    #[test]
    fn rate_measurement() {
        let a = Stamp {
            count: 10,
            time_ns: 1_000_000_000,
        };
        let b = Stamp {
            count: 70,
            time_ns: 2_000_000_000,
        };
        assert_eq!(rate_mhz(a, b), Some(60_000));
        assert_eq!(rate_mhz(b, a), None);
        assert_eq!(rate_mhz(a, a), None);
        assert!(within(60_000, 60_000, 10));
        assert!(within(60_500, 60_000, 10));
        assert!(!within(60_700, 60_000, 10));
        assert!(!within(59_000, 60_000, 10));
        assert_eq!(ts_khz(19_200, 1_000_000), Some(19_200));
        assert_eq!(ts_khz(0, 1_000_000), None);
        assert_eq!(rate_from_ticks(60, 2_586_000, 2_586), Some(60_000));
        assert_eq!(rate_from_ticks(0, 1, 1), None);
    }
}
