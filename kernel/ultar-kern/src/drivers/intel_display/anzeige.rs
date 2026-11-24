//! The driver behind the kernel's display layer (`grafik.rs`): lists the active output's EDID
//! modes and switches resolution with a mode set (stage B, display 14). The kernel console then
//! draws through the aperture (uncached CPU path into the buffer the display reads), so no cache
//! flush is needed for it.

use super::DEVICE;
use crate::efi::Framebuffer;
use crate::grafik::Anzeige;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// The Intel display as `Anzeige`.
pub struct IntelAnzeige;

impl Anzeige for IntelAnzeige {
    fn name(&self) -> &'static str {
        "intel-display"
    }

    fn herkunft(&self) -> &'static str {
        "hand"
    }

    fn modi(&self) -> Vec<(u32, u32)> {
        let g = DEVICE.lock();
        let mut v: Vec<(u32, u32)> = g
            .as_ref()
            .map(|d| {
                d.modes
                    .iter()
                    .map(|t| (t.hdisplay as u32, t.vdisplay as u32))
                    .collect()
            })
            .unwrap_or_default();
        v.dedup();
        v
    }

    /// Switches to `w`×`h`. The old console buffer stays mapped and on the screen until the new
    /// mode runs; it is released only in [`Anzeige::umgeschaltet`], after the console moved to
    /// the new buffer. A failed mode set restores the previous mode (see `stage_b.rs`): the
    /// console keeps drawing into the buffer that is shown.
    fn setzen(&mut self, w: u32, h: u32) -> Result<Framebuffer, String> {
        let mut g = DEVICE.lock();
        let dev = g.as_mut().ok_or("no Intel display device")?;
        let p = dev
            .pipes
            .iter()
            .position(|r| r.active)
            .ok_or("no active pipe")? as u8;
        let rt = dev.pipes[p as usize];
        if (rt.timing.hdisplay as u32, rt.timing.vdisplay as u32) == (w, h) {
            return crate::console::framebuffer()
                .ok_or_else(|| String::from("no console framebuffer"));
        }
        let t = dev
            .modes
            .iter()
            .copied()
            .find(|t| (t.hdisplay as u32, t.vdisplay as u32) == (w, h))
            .ok_or_else(|| format!("{w}x{h} is not a mode of this monitor"))?;
        let o = dev
            .outputs
            .iter()
            .copied()
            .find(|o| o.port == rt.port && o.dp)
            .ok_or("mode set is implemented for DisplayPort outputs only")?;
        let pr = dev.probe(o);
        let caps = pr.caps.ok_or("the monitor does not answer on AUX")?;
        let mut buf = dev.new_buffer(w, h).map_err(|e| format!("{e}"))?;
        let Some(cpu) = dev.aperture_addr(&buf) else {
            dev.free_buffer(buf);
            return Err(String::from("buffer outside the aperture"));
        };
        buf.draw(|_, _| 0);
        let r = dev.modeset_dp(p, o, t, &buf, caps);
        let fb = Framebuffer {
            base: cpu,
            width: w,
            height: h,
            stride: buf.pitch / 4,
            bgr: true,
        };
        // kept in both cases: after a failure the plane may still scan it out until the next
        // blank; it is released once nothing shows it
        dev.hold(buf);
        match r {
            Ok(()) => Ok(fb),
            Err(e) => {
                dev.release_unshown(&[]);
                Err(format!("mode set failed: {e}"))
            }
        }
    }

    fn umgeschaltet(&mut self) {
        // the console draws into the new buffer now: the old one may go
        if let Some(dev) = DEVICE.lock().as_mut() {
            dev.release_unshown(&[]);
        }
    }
}
