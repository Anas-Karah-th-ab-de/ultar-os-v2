//! The running device: what the firmware left (stage A takeover), scan-out buffers, flips,
//! vertical blanks, hot-plug and connector probing (EDID over AUX or GMBUS), and the DP mode set
//! (stage B) through the pure logic in `ultar_intel_display`.

use super::buffer::ScanBuf;
use super::mmio::Mmio;
use alloc::vec::Vec;
use ultar_intel_display::hotplug::{self, Tracker};
use ultar_intel_display::mode::Timing;
use ultar_intel_display::plane::{self, PlaneConfig};
use ultar_intel_display::readout::{self, ActivePipe};
use ultar_intel_display::regs::{irq, port};
use ultar_intel_display::vblank::{self, Counter, Stamp};
use ultar_intel_display::{cdclk, ggtt, power, vbt, Error, Hw, Platform};

/// A connector the board wires (from the VBT, or every Type-C port when there is none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Output {
    /// Port index.
    pub port: u8,
    /// DisplayPort signalling.
    pub dp: bool,
    /// HDMI signalling.
    pub hdmi: bool,
    /// GMBUS pin for HDMI DDC.
    pub ddc_pin: u8,
}

/// Runtime state of one pipe.
#[derive(Clone, Copy, Debug, Default)]
pub struct PipeRt {
    /// The pipe runs.
    pub active: bool,
    /// Port it feeds.
    pub port: u8,
    /// Timing (clock 0 when unknown).
    pub timing: Timing,
    /// What the primary plane shows.
    pub plane: Option<PlaneConfig>,
    /// The firmware's surface (to return to).
    pub fw_surf: Option<u32>,
    /// Frame counter extension.
    pub counter: Counter,
    /// Surface armed by a flip and not yet latched.
    pub pending: Option<u32>,
}

/// The state found at takeover.
#[derive(Clone, Debug, Default)]
pub struct BootFacts {
    /// Firmware pipes.
    pub found: Vec<ActivePipe>,
    /// Running pipes that do not decode (left alone) and why.
    pub skipped: Vec<(u8, Error)>,
    /// Core display clock (kHz).
    pub cdclk_khz: Option<u32>,
    /// `DC_STATE_EN` before the driver turned DC states off.
    pub dc_state_was: u32,
    /// `PWR_WELL_CTL2`.
    pub wells: u32,
    /// Rate of the display time base (`TIMESTAMP_CTR`) in kHz, measured against the CPU clock
    /// (0 = not known yet: blank times are then the time of reading).
    pub ts_khz: u32,
}

/// `BootFacts::ts_khz` when the display time base does not run at all.
pub const NO_TIME_BASE: u32 = u32::MAX;

/// Measures the display time base against the CPU clock over 20 ms (0 if it does not run – it
/// stands still while no pipe runs, seen in the VM before the mode set).
fn measure_ts_khz(mmio: &mut Mmio) -> u32 {
    let t0 = crate::arch::uptime_ns();
    let a = mmio.read(ultar_intel_display::regs::TIMESTAMP_CTR);
    crate::arch::sleep_us(20_000);
    let b = mmio.read(ultar_intel_display::regs::TIMESTAMP_CTR);
    let ns = crate::arch::uptime_ns() - t0;
    vblank::ts_khz(b.wrapping_sub(a), ns).unwrap_or(0)
}

/// The Intel display device.
pub struct Device {
    /// Registers.
    pub mmio: Mmio,
    /// Platform.
    pub plat: Platform,
    /// PCI address.
    pub pci: (u8, u8, u8),
    /// Wired connectors.
    pub outputs: Vec<Output>,
    /// Whether `outputs` came from the VBT.
    pub outputs_from_vbt: bool,
    /// Pipes.
    pub pipes: Vec<PipeRt>,
    /// GGTT space for kernel buffers.
    pub win: ggtt::Window,
    /// Attribute bits of the firmware's scan-out GGTT entry (copied into ours).
    pub pte_attrs: u64,
    /// GGTT entry of the scratch page (unused entries of the window point there).
    pub scratch: u64,
    /// Aperture (BAR2): CPU address and size; GGTT addresses below the size are reachable through it.
    pub aperture: Option<(u64, u64)>,
    /// Hot-plug change detection.
    pub hotplug: Tracker,
    /// What the firmware left (for the report and the boot display decision).
    pub boot: BootFacts,
    /// Kernel buffers that stay on a plane (released when the plane gets another one).
    pub kept: Vec<ScanBuf>,
    /// Modes of the active output (EDID), for the resolution switch.
    pub modes: Vec<Timing>,
}

/// What the fallible part of the takeover produced.
struct Taken {
    wells: u32,
    found: readout::Readout,
    win: ggtt::Window,
    pte_attrs: u64,
    scratch: u64,
}

/// Type-C ports of display 14 (DP on each) when the VBT is not available.
fn fallback_outputs(plat: &Platform) -> Vec<Output> {
    if !plat.is_xelpdp() {
        return Vec::new();
    }
    (port::PORT_TC1..=port::PORT_TC4)
        .map(|p| Output {
            port: p,
            dp: true,
            hdmi: false,
            ddc_pin: 0,
        })
        .collect()
}

/// Runtime state of every pipe from the firmware's running pipes.
fn pipes_of(plat: &Platform, found: &[ActivePipe]) -> Vec<PipeRt> {
    let mut pipes: Vec<PipeRt> = (0..plat.pipes).map(|_| PipeRt::default()).collect();
    for a in found {
        if let Some(rt) = pipes.get_mut(a.pipe as usize) {
            *rt = PipeRt {
                active: true,
                port: a.port,
                timing: a.timing,
                plane: a.plane.map(|p| p.cfg),
                fw_surf: a.plane.map(|p| p.cfg.surf),
                ..Default::default()
            };
        }
    }
    pipes
}

/// Connectors from the VBT (and whether they came from it), else every Type-C port.
fn outputs_of(vbt_img: Option<&[u8]>, plat: &Platform) -> (Vec<Output>, bool) {
    match vbt_img.map(vbt::outputs) {
        Some(Ok(o)) if !o.is_empty() => {
            let v = o.iter().map(|v| Output {
                port: v.port,
                dp: v.dp,
                hdmi: v.hdmi,
                ddc_pin: v.ddc_pin,
            });
            (v.collect(), true)
        }
        _ => (fallback_outputs(plat), false),
    }
}

impl Device {
    /// Takes over what the firmware runs, without changing what is on the screen. On failure
    /// the DC state setting is put back and nothing else was written.
    pub fn takeover(
        mut mmio: Mmio,
        plat: Platform,
        pci: (u8, u8, u8),
        vbt_img: Option<Vec<u8>>,
        aperture: Option<(u64, u64)>,
    ) -> Result<Device, Error> {
        if mmio.read(0x70008) == u32::MAX {
            return Err(Error::DeviceGone);
        }
        let dc_state_was = if plat.display_ver >= 11 {
            power::disable_dc_states(&mut mmio)
        } else {
            0
        };
        match Self::take(&mut mmio, &plat, aperture) {
            Ok(t) => Ok(Self::assemble(
                mmio,
                plat,
                pci,
                vbt_img,
                aperture,
                t,
                dc_state_was,
            )),
            Err(e) => {
                if plat.display_ver >= 11 {
                    mmio.write(power::DC_STATE_EN, dc_state_was);
                }
                Err(e)
            }
        }
    }

    /// Readout, GGTT window and scratch page (the part of the takeover that can fail).
    fn take(
        mmio: &mut Mmio,
        plat: &Platform,
        aperture: Option<(u64, u64)>,
    ) -> Result<Taken, Error> {
        let wells = mmio.read(power::PWR_WELL_CTL2);
        let found = readout::read_all(mmio, plat)?;
        let mut fw_end = 0u64;
        // without a firmware entry to copy: PAT index 2 (uncached) on display 14, as Linux uses
        // for scan-out; plain present entries before
        let mut pte_attrs = if plat.is_xelpdp() { 1 << 53 } else { 0 };
        for p in found.pipes.iter().filter_map(|a| a.plane) {
            fw_end = fw_end.max(p.cfg.surf as u64 + p.cfg.bytes());
            pte_attrs = ggtt::read_pte(mmio, p.cfg.surf);
        }
        let entries = ggtt::entries_from_ggc(mmio.read(ggtt::GGC)).max(1 << 18);
        let win = match aperture {
            Some((_, size)) if size >= 128 << 20 => {
                ggtt::Window::in_aperture(entries, fw_end, size, 32)?
            }
            _ => ggtt::Window::above_firmware(entries, fw_end, 32)?,
        };
        // one zeroed page behind every unused entry of the window (i915 does the same: the
        // display prefetches past a surface, older parts ignore the valid bit)
        let page = crate::mem::alloc_pages(1).ok_or(Error::NoSpace)?;
        super::buffer::flush(page, 4096);
        let scratch = match ggtt::pte(page, pte_attrs) {
            Ok(s) => s,
            Err(e) => {
                crate::mem::free_run(page, 1);
                return Err(e);
            }
        };
        ggtt::unmap(mmio, win.start(), win.pages(), scratch);
        Ok(Taken {
            wells,
            found,
            win,
            pte_attrs,
            scratch,
        })
    }

    fn assemble(
        mmio: Mmio,
        plat: Platform,
        pci: (u8, u8, u8),
        vbt_img: Option<Vec<u8>>,
        aperture: Option<(u64, u64)>,
        t: Taken,
        dc_state_was: u32,
    ) -> Device {
        let pipes = pipes_of(&plat, &t.found.pipes);
        let (outputs, outputs_from_vbt) = outputs_of(vbt_img.as_deref(), &plat);
        let boot = BootFacts {
            found: t.found.pipes,
            skipped: t.found.skipped,
            cdclk_khz: None,
            dc_state_was,
            wells: t.wells,
            ts_khz: 1000,
        };
        let mut dev = Device {
            mmio,
            plat,
            pci,
            outputs,
            outputs_from_vbt,
            pipes,
            win: t.win,
            pte_attrs: t.pte_attrs,
            scratch: t.scratch,
            aperture,
            hotplug: Tracker::default(),
            boot,
            kept: Vec::new(),
            modes: Vec::new(),
        };
        dev.boot.cdclk_khz = cdclk::read(&mut dev.mmio, plat.display_ver);
        if dev.pipes.iter().any(|r| r.active) {
            dev.boot.ts_khz = measure_ts_khz(&mut dev.mmio);
        }
        dev.irq_setup();
        if plat.is_xelpdp() {
            let ports: Vec<u8> = dev.outputs.iter().map(|o| o.port).collect();
            hotplug::enable(&mut dev.mmio, &ports);
            let mask = hotplug::live_mask(&mut dev.mmio, &ports);
            dev.hotplug.update(mask);
        }
        dev
    }

    /// Unmasks the pipe sources so that they latch in the identity registers (polled; the master
    /// interrupt stays off: the kernel has no MSI routing for PCI devices). The registers live in
    /// power well 1: after a cold display bring-up this has to be done again.
    fn irq_setup(&mut self) {
        for p in 0..self.plat.pipes {
            self.mmio.rmw(irq::pipe_imr(p), irq::PIPE_SOURCES, 0);
            self.mmio.write(irq::pipe_iir(p), irq::PIPE_SOURCES);
        }
    }

    /// The last vertical blank of an active pipe.
    pub fn vblank(&mut self, p: u8) -> Option<Stamp> {
        let rt = self.pipes.get(p as usize).copied()?;
        if !rt.active {
            return None;
        }
        if self.boot.ts_khz == 0 {
            // measured once while a pipe runs; NO_TIME_BASE when it does not run even then
            self.boot.ts_khz = match measure_ts_khz(&mut self.mmio) {
                0 => NO_TIME_BASE,
                k => k,
            };
        }
        let mut raw = vblank::read_raw(&mut self.mmio, p);
        if self.boot.ts_khz == NO_TIME_BASE {
            raw.now_ts = raw.frame_ts;
        }
        let now = crate::arch::uptime_ns();
        let st = vblank::stamp(
            &mut self.pipes[p as usize].counter,
            raw,
            now,
            self.boot.ts_khz,
        );
        Some(st)
    }

    /// Waits for the next vertical blank of `p` (frame counter change), at most `timeout_us`.
    pub fn wait_vblank(&mut self, p: u8, timeout_us: u64) -> Result<Stamp, Error> {
        let reg = ultar_intel_display::regs::pipe(p, ultar_intel_display::regs::FRMCOUNT);
        let start = self.mmio.read(reg);
        let t0 = crate::arch::uptime_ns() / 1000;
        while self.mmio.read(reg) == start {
            if crate::arch::uptime_ns() / 1000 - t0 > timeout_us {
                return Err(Error::Timeout { reg, last: start });
            }
            crate::arch::sleep_us(50);
        }
        self.vblank(p).ok_or(Error::NoActivePipe)
    }

    /// Shows the GGTT surface `surf` on `p` from the next blank on (same size and format).
    pub fn flip(&mut self, p: u8, surf: u32) -> Result<(), Error> {
        let rt = self
            .pipes
            .get_mut(p as usize)
            .ok_or(Error::Invalid("pipe"))?;
        if !rt.active || rt.plane.is_none() {
            return Err(Error::NoActivePipe);
        }
        plane::flip(&mut self.mmio, p, 0, surf)?;
        let rt = &mut self.pipes[p as usize];
        rt.pending = Some(surf);
        if let Some(pl) = rt.plane.as_mut() {
            pl.surf = surf;
        }
        Ok(())
    }

    /// If the flip of `p` was latched: the blank it happened at.
    pub fn flip_done(&mut self, p: u8) -> Option<Stamp> {
        let surf = self.pipes.get(p as usize)?.pending?;
        if !plane::latched(&mut self.mmio, p, 0, surf) {
            return None;
        }
        self.pipes[p as usize].pending = None;
        self.vblank(p)
    }
}
