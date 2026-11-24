//! Stage B of the running device: the DP mode set (a switch with rollback), turning a pipe off,
//! the EDID mode list of the active output, and the lifetime of kernel buffers on planes.

use super::buffer::ScanBuf;
use super::device::{Device, Output, PipeRt};
use alloc::vec::Vec;
use ultar_intel_display::dp::{dpcd, link};
use ultar_intel_display::mode::Timing;
use ultar_intel_display::modeset::{self, DpModeset};
use ultar_intel_display::plane::{self, PlaneConfig};
use ultar_intel_display::regs;
use ultar_intel_display::{edid, hotplug, optin, Error, Hw};

impl Device {
    /// Stage B: lights output `o` on pipe `p` with `timing`, showing `buf`.
    pub fn modeset_dp(
        &mut self,
        p: u8,
        o: Output,
        timing: Timing,
        buf: &ScanBuf,
        caps: dpcd::Caps,
    ) -> Result<(), Error> {
        let cfg = PlaneConfig {
            surf: buf.gtt,
            pitch: buf.pitch,
            width: buf.width,
            height: buf.height,
            format: plane::XRGB8888,
        };
        self.modeset_dp_plane(p, o, timing, cfg, caps)
    }

    /// Stage B with any plane content (a KMS framebuffer mapped into the GGTT). The new mode is
    /// checked before anything is touched; if it fails after the running mode was turned off,
    /// the running mode is lit again with its own surface (rollback).
    pub fn modeset_dp_plane(
        &mut self,
        p: u8,
        o: Output,
        timing: Timing,
        cfg: PlaneConfig,
        caps: dpcd::Caps,
    ) -> Result<(), Error> {
        if !self.plat.can_modeset() {
            return Err(Error::NotSupported("mode set on this display generation"));
        }
        let rt = self
            .pipes
            .get(p as usize)
            .copied()
            .ok_or(Error::Invalid("pipe"))?;
        let new = DpModeset {
            pipe: p,
            port: o.port,
            timing,
            plane: cfg,
            caps,
            bpc: 8,
        };
        let old = self.running_mode(p, rt, o.port, caps);
        if rt.active && old.is_none() {
            // nothing known to return to: plain switch
            modeset::precheck(&mut self.mmio, &new)?;
            self.disable_pipe(p);
        }
        let r = modeset::switch_dp(&mut self.mmio, old.as_ref(), &new);
        let out = self.note_switch(&new, r);
        self.rearm();
        out
    }

    /// Records the outcome of a switch to `new` in the pipe state.
    fn note_switch(
        &mut self,
        new: &DpModeset,
        r: Result<link::LinkConfig, modeset::SwitchError>,
    ) -> Result<(), Error> {
        let (p, t) = (new.pipe, new.timing);
        match r {
            Ok(link) => {
                crate::kprintln!(
                    "[intel-display] pipe {} on port {} up: {}x{} {} kHz, link {} kHz x{}",
                    p,
                    new.port,
                    t.hdisplay,
                    t.vdisplay,
                    t.clock,
                    link.rate,
                    link.lanes
                );
                self.pipes[p as usize] = PipeRt {
                    active: true,
                    port: new.port,
                    timing: t,
                    plane: Some(new.plane),
                    ..Default::default()
                };
                Ok(())
            }
            Err((e, Some(Ok(_)))) => {
                crate::kprintln!("[intel-display] mode set failed ({e}); previous mode restored");
                Err(e)
            }
            Err((e, Some(Err(e2)))) => {
                crate::kprintln!(
                    "[intel-display] mode set failed ({e}); previous mode not restored: {e2}"
                );
                self.pipes[p as usize] = PipeRt::default();
                Err(e)
            }
            Err((e, None)) => Err(e),
        }
    }

    /// The mode pipe `p` runs as a mode set request (for the rollback), if it is fully known.
    fn running_mode(
        &mut self,
        p: u8,
        rt: PipeRt,
        new_port: u8,
        caps: dpcd::Caps,
    ) -> Option<DpModeset> {
        let plane = rt.plane.filter(|_| rt.active && rt.timing.clock != 0)?;
        let caps = if rt.port == new_port {
            caps
        } else {
            modeset::read_caps(&mut self.mmio, rt.port, self.plat.display_ver).ok()?
        };
        Some(DpModeset {
            pipe: p,
            port: rt.port,
            timing: rt.timing,
            plane,
            caps,
            bpc: 8,
        })
    }

    /// The mode set may have powered the display core up: interrupt masks and hot-plug detection
    /// written at takeover were lost then.
    fn rearm(&mut self) {
        for p in 0..self.plat.pipes {
            self.mmio
                .rmw(regs::irq::pipe_imr(p), regs::irq::PIPE_SOURCES, 0);
            self.mmio
                .write(regs::irq::pipe_iir(p), regs::irq::PIPE_SOURCES);
        }
        let ports: Vec<u8> = self.outputs.iter().map(|o| o.port).collect();
        hotplug::enable(&mut self.mmio, &ports);
    }

    /// Turns pipe `p` off (display 9: the plane only).
    pub fn disable_pipe(&mut self, p: u8) {
        let Some(rt) = self.pipes.get(p as usize).copied() else {
            return;
        };
        if !rt.active {
            return;
        }
        if self.plat.can_modeset() {
            if let Err(e) = modeset::disable(&mut self.mmio, p, rt.port) {
                crate::kprintln!("[intel-display] pipe {p} off: {e}");
            }
            modeset::tc_disconnect(&mut self.mmio, rt.port);
        } else {
            plane::disable(&mut self.mmio, p, 0);
        }
        self.pipes[p as usize] = PipeRt::default();
    }

    /// Reads the EDID modes of the output the first active pipe drives (resolution switch).
    pub fn collect_modes(&mut self) {
        let Some(rt) = self.pipes.iter().find(|r| r.active).copied() else {
            return;
        };
        let Some(o) = self.outputs.iter().copied().find(|o| o.port == rt.port) else {
            return;
        };
        let pr = self.probe(o);
        let mut modes: Vec<Timing> = pr
            .edid
            .as_deref()
            .and_then(|e| edid::parse(e).ok())
            .map(|i| i.timings)
            .unwrap_or_default();
        modes.retain(|t| t.validate().is_ok());
        if rt.timing.clock != 0 && !modes.iter().any(|m| m.same_signal(&rt.timing)) {
            modes.insert(0, rt.timing);
        }
        self.modes = modes;
    }

    /// Keeps `b` alive (a plane shows it or will); nothing is released here.
    pub fn hold(&mut self, b: ScanBuf) {
        self.kept.push(b);
    }

    /// Releases kept buffers that no plane shows, latched or armed. `also_shown`: GGTT addresses
    /// that must stay although no plane shows them (none in the driver itself).
    pub fn release_unshown(&mut self, also_shown: &[u32]) {
        let mut shown: Vec<u32> = (0..self.plat.pipes)
            .map(|p| self.mmio.read(regs::plane(p, 0, regs::PLANE_SURFLIVE)) & !0xfff)
            .collect();
        shown.extend(self.pipes.iter().filter_map(|r| r.pending));
        shown.extend(self.pipes.iter().filter_map(|r| r.plane.map(|p| p.surf)));
        shown.extend_from_slice(also_shown);
        let kept: Vec<u32> = self.kept.iter().map(|b| b.gtt).collect();
        for g in optin::releasable(&kept, &shown) {
            if let Some(i) = self.kept.iter().position(|b| b.gtt == g) {
                let b = self.kept.swap_remove(i);
                b.release(&mut self.mmio, &mut self.win, self.scratch);
            }
        }
    }

    /// Keeps `b` and frees kept buffers no plane shows any more.
    pub fn keep(&mut self, b: ScanBuf) {
        self.hold(b);
        self.release_unshown(&[]);
    }
}
