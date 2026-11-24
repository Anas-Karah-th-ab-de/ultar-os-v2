//! Display timings and their transcoder register form.

use crate::error::{Error, Result};
use crate::hw::Hw;
use crate::regs::{self, trans};

/// `DRM_MODE_FLAG_PHSYNC`.
pub const FLAG_PHSYNC: u32 = 1;
/// `DRM_MODE_FLAG_NHSYNC`.
pub const FLAG_NHSYNC: u32 = 2;
/// `DRM_MODE_FLAG_PVSYNC`.
pub const FLAG_PVSYNC: u32 = 4;
/// `DRM_MODE_FLAG_NVSYNC`.
pub const FLAG_NVSYNC: u32 = 8;
/// `DRM_MODE_FLAG_INTERLACE`.
pub const FLAG_INTERLACE: u32 = 0x10;

/// A display timing (fields as Linux' `drm_mode_modeinfo` and the KMS core's `Timing`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timing {
    /// Pixel clock in kHz.
    pub clock: u32,
    /// Active width.
    pub hdisplay: u16,
    /// Horizontal sync start.
    pub hsync_start: u16,
    /// Horizontal sync end.
    pub hsync_end: u16,
    /// Horizontal total.
    pub htotal: u16,
    /// Active height.
    pub vdisplay: u16,
    /// Vertical sync start.
    pub vsync_start: u16,
    /// Vertical sync end.
    pub vsync_end: u16,
    /// Vertical total.
    pub vtotal: u16,
    /// `FLAG_*`.
    pub flags: u32,
}

/// Transcoder timing registers of one timing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingRegs {
    /// HTOTAL, HBLANK, HSYNC, VTOTAL, VBLANK, VSYNC in this order.
    pub vals: [u32; 6],
}

const fn pair(lo: u16, hi: u16) -> u32 {
    ((hi.saturating_sub(1) as u32) << 16) | lo.saturating_sub(1) as u32
}

fn unpair(v: u32) -> (u16, u16) {
    (((v & 0xffff) + 1) as u16, ((v >> 16) + 1) as u16)
}

impl Timing {
    /// Refresh rate in millihertz.
    pub fn refresh_mhz(&self) -> u32 {
        let total = self.htotal as u64 * self.vtotal as u64;
        (self.clock as u64 * 1_000_000)
            .checked_div(total)
            .unwrap_or(0) as u32
    }

    /// Length of a frame in nanoseconds.
    pub fn frame_ns(&self) -> u64 {
        if self.clock == 0 {
            0
        } else {
            self.htotal as u64 * self.vtotal as u64 * 1_000_000 / self.clock as u64
        }
    }

    /// Checks that the timing is consistent and something the transcoder can run.
    pub fn validate(&self) -> Result<()> {
        let h_ok = 0 < self.hdisplay
            && self.hdisplay < self.hsync_start
            && self.hsync_start < self.hsync_end
            && self.hsync_end <= self.htotal;
        let v_ok = 0 < self.vdisplay
            && self.vdisplay < self.vsync_start
            && self.vsync_start < self.vsync_end
            && self.vsync_end <= self.vtotal;
        if !h_ok || !v_ok {
            return Err(Error::Invalid("timing order"));
        }
        if self.flags & FLAG_INTERLACE != 0 {
            return Err(Error::NotSupported("interlaced modes"));
        }
        if self.htotal > 8192 || self.vtotal > 8192 || self.hdisplay > 5120 || self.vdisplay > 4096
        {
            return Err(Error::Invalid("timing too large"));
        }
        if !(25_000..=1_300_000).contains(&self.clock) {
            return Err(Error::Invalid("pixel clock"));
        }
        Ok(())
    }

    /// The register values (no borders: blank = active .. total). Meant for timings that passed
    /// [`Timing::validate`]; fields of an invalid timing saturate instead of wrapping.
    pub fn regs(&self) -> TimingRegs {
        TimingRegs {
            vals: [
                pair(self.hdisplay, self.htotal),
                pair(self.hdisplay, self.htotal),
                pair(self.hsync_start, self.hsync_end),
                pair(self.vdisplay, self.vtotal),
                pair(self.vdisplay, self.vtotal),
                pair(self.vsync_start, self.vsync_end),
            ],
        }
    }

    /// Timing from register values, pixel clock and sync flags from elsewhere.
    pub fn from_regs(r: &TimingRegs, clock: u32, flags: u32) -> Timing {
        let (hdisplay, htotal) = unpair(r.vals[0]);
        let (hsync_start, hsync_end) = unpair(r.vals[2]);
        let (vdisplay, vtotal) = unpair(r.vals[3]);
        let (vsync_start, vsync_end) = unpair(r.vals[5]);
        Timing {
            clock,
            hdisplay,
            hsync_start,
            hsync_end,
            htotal,
            vdisplay,
            vsync_start,
            vsync_end,
            vtotal,
            flags,
        }
    }

    /// Whether two timings drive the monitor identically (clock within 0.5 %).
    pub fn same_signal(&self, o: &Timing) -> bool {
        let geometry = (
            self.hdisplay,
            self.hsync_start,
            self.hsync_end,
            self.htotal,
            self.vdisplay,
            self.vsync_start,
            self.vsync_end,
            self.vtotal,
        ) == (
            o.hdisplay,
            o.hsync_start,
            o.hsync_end,
            o.htotal,
            o.vdisplay,
            o.vsync_start,
            o.vsync_end,
            o.vtotal,
        );
        let diff = self.clock.abs_diff(o.clock) as u64;
        geometry && diff * 200 <= self.clock.max(1) as u64
    }
}

/// Offsets of the six timing registers relative to the transcoder block.
const OFFS: [u32; 6] = [
    regs::HTOTAL,
    regs::HBLANK,
    regs::HSYNC,
    regs::VTOTAL,
    regs::VBLANK,
    regs::VSYNC,
];

/// Reads the timing registers of transcoder `t`.
pub fn read_regs<H: Hw>(hw: &mut H, t: u8) -> TimingRegs {
    let mut vals = [0; 6];
    for (v, off) in vals.iter_mut().zip(OFFS) {
        *v = hw.read(trans(t, off));
    }
    TimingRegs { vals }
}

/// `TRANS_SET_CONTEXT_LATENCY` (display 13+).
pub const fn set_context_latency(t: u8) -> u32 {
    trans(t, 0x7c)
}

/// Programs the timing of transcoder `t` and its pipe source size (display 13+ semantics: the
/// vertical blank start is set through the context latency, 0 here).
pub fn write_regs<H: Hw>(hw: &mut H, t: u8, tm: &Timing, display_ver: u8) {
    let mut r = tm.regs();
    if display_ver >= 13 {
        hw.write(set_context_latency(t), 0);
        // VBLANK_START is unused by the hardware: 1 makes it stand out in dumps (as Linux does)
        r.vals[4] = (r.vals[4] & 0xffff_0000) | pair(1, 1) & 0xffff;
    }
    hw.write(trans(t, regs::VSYNCSHIFT), 0);
    for (v, off) in r.vals.iter().zip(OFFS) {
        hw.write(trans(t, off), *v);
    }
    hw.write(trans(t, regs::PIPESRC), pair(tm.vdisplay, tm.hdisplay));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;

    use crate::sim::fhd;

    #[test]
    fn registers_match_the_captured_dump() {
        let r = fhd().regs();
        assert_eq!(r.vals[0], 0x0897_077f); // HTOTAL_A
        assert_eq!(r.vals[1], 0x0897_077f); // HBLANK_A
        assert_eq!(r.vals[2], 0x0803_07d7); // HSYNC_A
        assert_eq!(r.vals[3], 0x0464_0437); // VTOTAL_A
        assert_eq!(r.vals[5], 0x0440_043b); // VSYNC_A
        let back = Timing::from_regs(&r, 148_500, fhd().flags);
        assert_eq!(back, fhd());
    }

    #[test]
    fn write_regs_uses_context_latency_on_display_14() {
        let mut s = Sim::new();
        write_regs(&mut s, 0, &fhd(), 14);
        assert_eq!(s.get(0x60010), 0x0464_0000); // VBLANK_A as captured
        assert_eq!(s.get(0x6001c), 0x077f_0437); // PIPESRC_A as captured
        assert_eq!(s.get(0x6007c), 0);
        let mut s9 = Sim::new();
        write_regs(&mut s9, 1, &fhd(), 9);
        assert_eq!(s9.get(0x61010), 0x0464_0437);
        assert!(s9.writes_to(0x6107c).is_empty());
        assert_eq!(read_regs(&mut s9, 1), fhd().regs());
    }

    #[test]
    fn refresh_and_frame_time() {
        let t = fhd();
        assert_eq!(t.refresh_mhz(), 60_000);
        assert_eq!(t.frame_ns(), 16_666_666);
        assert_eq!(Timing::default().refresh_mhz(), 0);
        assert_eq!(Timing::default().frame_ns(), 0);
    }

    #[test]
    fn validation() {
        assert_eq!(fhd().validate(), Ok(()));
        let mut bad = fhd();
        bad.hsync_start = 1900;
        assert_eq!(bad.validate(), Err(Error::Invalid("timing order")));
        let mut il = fhd();
        il.flags |= FLAG_INTERLACE;
        assert_eq!(il.validate(), Err(Error::NotSupported("interlaced modes")));
        let mut slow = fhd();
        slow.clock = 1000;
        assert_eq!(slow.validate(), Err(Error::Invalid("pixel clock")));
        let mut big = fhd();
        big.hdisplay = 6000;
        big.hsync_start = 6100;
        big.hsync_end = 6200;
        big.htotal = 6300;
        assert_eq!(big.validate(), Err(Error::Invalid("timing too large")));
    }

    #[test]
    fn same_signal_tolerates_rounding() {
        let a = fhd();
        let mut b = fhd();
        b.clock = 148_352; // 59.94 Hz variant: 0.1 % off
        assert!(a.same_signal(&b));
        b.clock = 140_000;
        assert!(!a.same_signal(&b));
        let mut c = fhd();
        c.vtotal = 1126;
        assert!(!a.same_signal(&c));
    }
}
