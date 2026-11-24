//! Reading back what the firmware (UEFI GOP) or an earlier driver left running: active pipes,
//! their port, link and timing, and their primary plane. Nothing is written here.

use crate::dp::link::MN;
use crate::error::{Error, Result};
use crate::hw::{field, Hw};
use crate::mode::{self, Timing, FLAG_NHSYNC, FLAG_NVSYNC, FLAG_PHSYNC, FLAG_PVSYNC};
use crate::plane::{self, FwPlane};
use crate::platform::Platform;
use crate::regs::{self, ddi, pipe, trans};
use alloc::vec::Vec;

/// One running pipe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivePipe {
    /// Pipe (= transcoder) index.
    pub pipe: u8,
    /// Port it feeds.
    pub port: u8,
    /// Signal kind.
    pub mode: ddi::DdiMode,
    /// Bits per colour.
    pub bpc: u8,
    /// Lanes.
    pub lanes: u8,
    /// Link M/N (DP).
    pub mn: Option<MN>,
    /// Port clock in kHz when the hardware tells it (display 14: `DDI_CLK_VALFREQ`).
    pub port_clock: Option<u32>,
    /// Timing; `clock` is 0 when it could not be derived.
    pub timing: Timing,
    /// The primary plane, if enabled and usable.
    pub plane: Option<FwPlane>,
    /// Why the primary plane cannot be taken over (it stays as it is).
    pub plane_error: Option<Error>,
}

fn read_mn<H: Hw>(hw: &mut H, t: u8) -> MN {
    let dm = hw.read(trans(t, regs::DATA_M1));
    MN {
        tu: field(dm, regs::TU_SIZE_MASK) + 1,
        data_m: dm & regs::MN_MASK,
        data_n: hw.read(trans(t, regs::DATA_N1)) & regs::MN_MASK,
        link_m: hw.read(trans(t, regs::LINK_M1)) & regs::MN_MASK,
        link_n: hw.read(trans(t, regs::LINK_N1)) & regs::MN_MASK,
    }
}

/// Pixel clock from link M/N and the port clock (DP: pixel = port × M / N).
pub fn pixel_clock(mn: &MN, port_clock: u32) -> u32 {
    if mn.link_n == 0 {
        0
    } else {
        (port_clock as u64 * mn.link_m as u64).div_ceil(mn.link_n as u64) as u32
    }
}

/// Reads pipe `p`; `None` when it does not run.
pub fn read_pipe<H: Hw>(hw: &mut H, plat: &Platform, p: u8) -> Result<Option<ActivePipe>> {
    let conf = hw.read(pipe(p, regs::TRANSCONF));
    if conf == u32::MAX {
        return Err(Error::DeviceGone);
    }
    if conf & regs::TRANSCONF_ENABLE == 0 || conf & regs::TRANSCONF_STATE == 0 {
        return Ok(None);
    }
    let func = hw.read(ddi::func_ctl(p));
    if func & ddi::FUNC_ENABLE == 0 {
        return Ok(None);
    }
    let port = ddi::port_from_bits(func, plat.display_ver)
        .ok_or(Error::Invalid("transcoder without port"))?;
    let mode = ddi::DdiMode::from_bits(func);
    let lanes = field(func, ddi::FUNC_WIDTH_MASK) as u8 + 1;
    let mut flags = if func & ddi::FUNC_PHSYNC != 0 {
        FLAG_PHSYNC
    } else {
        FLAG_NHSYNC
    };
    flags |= if func & ddi::FUNC_PVSYNC != 0 {
        FLAG_PVSYNC
    } else {
        FLAG_NVSYNC
    };
    let is_dp = matches!(mode, ddi::DdiMode::DpSst | ddi::DdiMode::DpMst);
    let mn = is_dp.then(|| read_mn(hw, p));
    let port_clock = if plat.display_ver >= 14 {
        Some(hw.read(ddi::clk_valfreq(port))).filter(|v| *v != 0)
    } else {
        None
    };
    let clock = match (mn, port_clock) {
        (Some(mn), Some(pc)) => pixel_clock(&mn, pc),
        (None, Some(pc)) => pc, // TMDS: port clock = pixel clock at 8 bpc
        _ => 0,
    };
    let timing = Timing::from_regs(&mode::read_regs(hw, p), clock, flags);
    let (plane, plane_error) = match plane::read_fw(hw, p, 0) {
        Ok(pl) => (pl, None),
        Err(e) => (None, Some(e)),
    };
    Ok(Some(ActivePipe {
        pipe: p,
        port,
        mode,
        bpc: ddi::bpc_from_bits(func),
        lanes,
        mn,
        port_clock,
        timing,
        plane,
        plane_error,
    }))
}

/// What [`read_all`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Readout {
    /// Running pipes that decode.
    pub pipes: Vec<ActivePipe>,
    /// Running pipes that do not decode (left as they are) and why.
    pub skipped: Vec<(u8, Error)>,
}

/// All running pipes. A pipe that does not decode is skipped (and named) instead of failing the
/// others; only a device that reads all ones fails the whole readout.
pub fn read_all<H: Hw>(hw: &mut H, plat: &Platform) -> Result<Readout> {
    let mut r = Readout::default();
    for p in 0..plat.pipes {
        match read_pipe(hw, plat, p) {
            Ok(Some(a)) => r.pipes.push(a),
            Ok(None) => {}
            Err(Error::DeviceGone) => return Err(Error::DeviceGone),
            Err(e) => r.skipped.push((p, e)),
        }
    }
    Ok(r)
}

#[cfg(test)]
pub(crate) fn load_beelink(s: &mut crate::sim::Sim) {
    // values of the register dump taken under Linux (pipe A on TC4, DP SST x4, 1080p60)
    for (r, v) in [
        (0x70008, 0xc000_0000u32),
        (0x60400, 0xba03_0006),
        (0x60000, 0x0897_077f),
        (0x60004, 0x0897_077f),
        (0x60008, 0x0803_07d7),
        (0x6000c, 0x0464_0437),
        (0x60010, 0x0464_0000),
        (0x60014, 0x0440_043b),
        (0x6001c, 0x077f_0437),
        (0x60030, 0x7e58_0000),
        (0x60034, 0x0080_0000),
        (0x60040, 0x0007_5555),
        (0x60044, 0x0008_0000),
        (0x70180, 0x8400_0000),
        (0x70188, 0x12ac_0078),
        (0x70190, (1079 << 16) | 1919),
        (0x7019c, 0x0175_b000),
        (0x64630, 162_000),
    ] {
        s.set(r, v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::lookup;
    use crate::sim::{fhd, Sim};

    #[test]
    fn reads_the_beelink_state() {
        let mut s = Sim::new();
        load_beelink(&mut s);
        let plat = lookup(0x7d51).unwrap();
        let pipes = read_all(&mut s, &plat).unwrap().pipes;
        assert_eq!(pipes.len(), 1);
        let a = &pipes[0];
        assert_eq!(
            (a.pipe, a.port, a.mode, a.bpc, a.lanes),
            (0, 6, ddi::DdiMode::DpSst, 8, 4)
        );
        assert_eq!(a.port_clock, Some(162_000));
        assert_eq!(a.timing.clock, 148_500);
        assert_eq!(a.timing, fhd());
        assert_eq!(a.plane.unwrap().cfg.surf, 0x0175_b000);
        assert!(a.plane_error.is_none());
        assert!(s.writes.is_empty(), "readout must not write");
    }

    #[test]
    fn inactive_gone_and_odd_states() {
        let plat = lookup(0x7d51).unwrap();
        let mut s = Sim::new();
        assert_eq!(read_all(&mut s, &plat).unwrap(), Readout::default());
        s.set(0x70008, u32::MAX);
        assert_eq!(read_all(&mut s, &plat), Err(Error::DeviceGone));
        let mut t = Sim::new();
        load_beelink(&mut t);
        t.set(0x70180, 0x8400_0400); // X-tiled firmware plane
        let a = read_pipe(&mut t, &plat, 0).unwrap().unwrap();
        assert!(a.plane.is_none());
        assert_eq!(a.plane_error, Some(Error::UnsupportedPlane(0x8400_0400)));
        t.set(0x60400, 0x8000_0000); // enabled without port
        assert_eq!(
            read_pipe(&mut t, &plat, 0),
            Err(Error::Invalid("transcoder without port"))
        );
        // one odd pipe does not hide the good ones
        t.set(0x71008, 0xc000_0000);
        t.set(0x61400, 0xba03_0006);
        let r = read_all(&mut t, &plat).unwrap();
        assert_eq!(
            r.pipes.iter().map(|a| a.pipe).collect::<Vec<_>>(),
            alloc::vec![1]
        );
        assert_eq!(
            r.skipped,
            alloc::vec![(0, Error::Invalid("transcoder without port"))]
        );
        t.set(0x60400, 0);
        assert_eq!(read_pipe(&mut t, &plat, 0), Ok(None));
    }

    #[test]
    fn display9_has_no_port_clock() {
        let mut s = Sim::new();
        load_beelink(&mut s);
        s.set(0x60400, 0x8000_0000 | (2 << 28) | (2 << 24) | (3 << 1));
        let plat = lookup(0x9bc4).unwrap();
        let a = read_pipe(&mut s, &plat, 0).unwrap().unwrap();
        assert_eq!(a.port, 2);
        assert_eq!(a.timing.clock, 0);
        assert_eq!(
            pixel_clock(
                &MN {
                    tu: 64,
                    data_m: 0,
                    data_n: 0,
                    link_m: 1,
                    link_n: 0
                },
                1
            ),
            0
        );
    }
}
