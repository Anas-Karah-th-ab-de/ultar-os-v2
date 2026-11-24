//! Display power wells and DC states. The driver keeps every well the firmware turned on and only
//! adds the ones a new pipe needs; DC5/DC6 (power saving with DMC firmware) are turned off, because
//! without the DMC firmware the registers of a well in DC state are not restored.

use crate::error::Result;
use crate::hw::Hw;

/// `HSW_PWR_WELL_CTL2`: driver request register of the main power wells.
pub const PWR_WELL_CTL2: u32 = 0x45404;
/// `DC_STATE_EN`.
pub const DC_STATE_EN: u32 = 0x45504;
/// `DC_STATE_EN`: DC5/DC6/DC3CO field.
pub const DC_STATE_MASK: u32 = 0b11;
/// `DBUF_CTL` of the DBUF slices S1..S4 (display 14: 4 slices of 1024 blocks; S1 is at 0x45008 –
/// the first hardware run used 0x44fe8 = S2 for S1 and the pipe underran from a cold start).
pub const DBUF_CTL: [u32; 4] = [0x45008, 0x44fe8, 0x44300, 0x44304];
/// `DBUF_CTL_S1`.
pub const DBUF_CTL_S1: u32 = DBUF_CTL[0];
/// `DBUF_CTL_S2`.
pub const DBUF_CTL_S2: u32 = DBUF_CTL[1];
/// DBUF blocks per slice on display 14.
pub const DBUF_SLICE_BLOCKS: u32 = 1024;
/// DBUF slice power request.
pub const DBUF_POWER_REQUEST: u32 = 1 << 31;
/// DBUF slice power state.
pub const DBUF_POWER_STATE: u32 = 1 << 30;

/// Power well index of display 12+ (`ICL_PW_CTL_IDX_*`, `XELPD_PW_CTL_IDX_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Well {
    /// PW1: display core.
    Pw1,
    /// PW2: Type-C and transcoder B–D logic.
    Pw2,
    /// Pipe A … D wells (display 13+).
    Pipe(u8),
}

impl Well {
    /// Index in the control register.
    pub fn idx(self) -> u32 {
        match self {
            Well::Pw1 => 0,
            Well::Pw2 => 1,
            Well::Pipe(p) => 5 + p as u32,
        }
    }
    /// Request bit.
    pub fn req(self) -> u32 {
        2 << (2 * self.idx())
    }
    /// State bit.
    pub fn state(self) -> u32 {
        1 << (2 * self.idx())
    }
}

/// Whether a well is on.
pub fn is_on<H: Hw>(hw: &mut H, w: Well) -> bool {
    hw.read(PWR_WELL_CTL2) & w.state() != 0
}

/// Turns a well on (if it is not) and waits for its state.
pub fn enable<H: Hw>(hw: &mut H, w: Well) -> Result<()> {
    if is_on(hw, w) {
        return Ok(());
    }
    hw.rmw(PWR_WELL_CTL2, 0, w.req());
    hw.wait_set(PWR_WELL_CTL2, w.state(), 5000).map(|_| ())
}

/// The wells pipe `p` needs on display 13+ (driving a Type-C port).
pub fn wells_for_pipe(p: u8) -> [Well; 3] {
    [Well::Pw1, Well::Pw2, Well::Pipe(p)]
}

/// Turns DC states off (keeps the other bits) and returns the old value.
pub fn disable_dc_states<H: Hw>(hw: &mut H) -> u32 {
    let old = hw.read(DC_STATE_EN);
    if old & DC_STATE_MASK != 0 {
        hw.write(DC_STATE_EN, old & !DC_STATE_MASK);
    }
    old
}

/// Slices (bit n = S(n+1)) that DDB blocks `[start, end)` lie in.
pub fn dbuf_slices_of(start: u32, end: u32) -> u8 {
    (0..4u32)
        .filter(|s| start < (s + 1) * DBUF_SLICE_BLOCKS && end > s * DBUF_SLICE_BLOCKS)
        .fold(0, |m, s| m | 1 << s)
}

/// Makes sure DBUF slice `slice` (0 = S1 … 3 = S4) has power.
pub fn dbuf_on<H: Hw>(hw: &mut H, slice: u8) -> Result<()> {
    let reg = DBUF_CTL[(slice as usize).min(3)];
    if hw.read(reg) & DBUF_POWER_STATE != 0 {
        return Ok(());
    }
    hw.rmw(reg, 0, DBUF_POWER_REQUEST);
    hw.wait_set(reg, DBUF_POWER_STATE, 1000).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;
    use alloc::boxed::Box;

    #[test]
    fn captured_wells() {
        // PWR_WELL_CTL2 on the Beelink under Linux: 0xc0f = PW1, PW2, PW_A on
        let mut s = Sim::new();
        s.set(PWR_WELL_CTL2, 0xc0f);
        assert!(
            is_on(&mut s, Well::Pw1) && is_on(&mut s, Well::Pw2) && is_on(&mut s, Well::Pipe(0))
        );
        assert!(!is_on(&mut s, Well::Pipe(1)));
        for w in wells_for_pipe(0) {
            enable(&mut s, w).unwrap();
        }
        assert!(s.writes.is_empty());
        assert_eq!(Well::Pipe(3).idx(), 8);
    }

    #[test]
    fn enable_requests_and_waits() {
        let mut s = Sim::new();
        s.on_write(
            PWR_WELL_CTL2,
            Box::new(|regs, reg, v| {
                regs.insert(reg, v | (v & 0xaaaa_aaaa) >> 1);
            }),
        );
        enable(&mut s, Well::Pipe(1)).unwrap();
        assert_eq!(s.get(PWR_WELL_CTL2), 0b11 << 12);
        let mut dead = Sim::new();
        assert!(enable(&mut dead, Well::Pw2).is_err());
    }

    #[test]
    fn dc_states_and_dbuf() {
        let mut s = Sim::new();
        s.set(DC_STATE_EN, 0x1000_0002);
        assert_eq!(disable_dc_states(&mut s), 0x1000_0002);
        assert_eq!(s.get(DC_STATE_EN), 0x1000_0000);
        assert_eq!(disable_dc_states(&mut s), 0x1000_0000);
        assert_eq!(s.writes.len(), 1);
        s.set(DBUF_CTL_S1, DBUF_POWER_STATE);
        dbuf_on(&mut s, 0).unwrap();
        s.set(DBUF_CTL_S1, 0);
        s.after_reads(DBUF_CTL_S1, 2, DBUF_POWER_REQUEST | DBUF_POWER_STATE);
        dbuf_on(&mut s, 0).unwrap();
        assert!(dbuf_on(&mut s, 1).is_err());
        assert_eq!(s.get(DBUF_CTL_S2), DBUF_POWER_REQUEST);
        assert_eq!(DBUF_CTL_S1, 0x45008);
        assert_eq!(dbuf_slices_of(0, 2048), 0b0011);
        assert_eq!(dbuf_slices_of(2048, 4096), 0b1100);
        assert_eq!(dbuf_slices_of(1000, 1100), 0b0011);
        assert_eq!(dbuf_slices_of(0, 0), 0);
    }
}
