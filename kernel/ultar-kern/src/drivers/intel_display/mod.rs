//! Intel display driver of the Ultar kernel (display 14: Meteor Lake / Arrow Lake, e.g. the Arc
//! 140T 8086:7d51 of the Beelink GTi15 Ultra; display 9 Comet Lake: takeover only).
//!
//! Stage A: takes over what the UEFI GOP set up without a visible change (pipes, ports, plane),
//! maps kernel buffers into the GGTT and flips the primary plane to them, reads vertical blanks
//! from the pipe's frame counter and time stamp, checks scan-out with the pipe CRC, watches
//! hot-plug and reads EDID over AUX/DDC. Stage B: DisplayPort mode set on the Type-C ports.
//! The logic lives in `drivers/kernel/intel-display` (pure, tested on the host); this module adds
//! MMIO, memory and the kernel interfaces. Documentation: `docs/integration/intel-display.md`.
//!
//! Opt-in: the driver writes to the hardware only when the machine opted in (boot option
//! `ultar.intel_display=on`, the file `\ultar\intel-display` with `on` on the start partition,
//! or the device id in `optin::ALLOWLIST`, empty for now). Otherwise it only reads what the
//! firmware runs, reports it, and the console stays on the firmware framebuffer.

pub mod anzeige;
pub mod buffer;
pub mod connector;
pub mod device;
pub mod gt;
pub mod gt_exec;
pub mod gt_irq;
pub mod gt_job;
pub mod gt_run;
pub mod gt_slpc;
pub mod gt_uapi;
pub mod gt_mem;
pub mod mmio;
pub mod opregion;
pub mod selftest;
pub mod stage_b;

use crate::driver::{Driver, Result};
use crate::pci::PciDev;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use core::sync::atomic::{AtomicBool, Ordering};
use device::Device;
use mmio::Mmio;
use spin::Mutex;
use ultar_intel_display::{optin, platform, readout, Platform};

/// The driver.
pub struct Drv;
/// The driver instance for `drivers::ALL`.
pub static DRV: Drv = Drv;

/// The bound device (one integrated GPU per machine).
pub static DEVICE: Mutex<Option<Device>> = Mutex::new(None);

/// Set when the boot option or the opt-in file was found (firmware phase).
static OPTED_IN: AtomicBool = AtomicBool::new(false);

/// Takes `ultar.intel_display=on` out of the first `n` UEFI load option units (the A/B start
/// chain refuses unknown `ultar.` words) and returns the remaining length.
pub fn take_boot_option(options: &mut [u16], n: usize) -> usize {
    // runs before the heap exists: in place, no allocation
    let n = n.min(options.len());
    let (len, on) = optin::take_option(&mut options[..n]);
    if on {
        OPTED_IN.store(true, Ordering::Relaxed);
    }
    len
}

/// Opts in when the content of the file `optin::FILE_PATH` says so.
pub fn opt_in_file(content: &[u8]) {
    if optin::file_says_on(content) {
        OPTED_IN.store(true, Ordering::Relaxed);
    }
}

/// Without opt-in: identification and readout only (no PCI command, no register write).
fn read_only(d: &PciDev, plat: Platform, bar0: u64, size: u64) -> String {
    let how =
        "opt in with the boot option ultar.intel_display=on or the file \\ultar\\intel-display";
    if d.r32(0x04) & 0x2 == 0 {
        return format!(
            "{} display {}: not driven, memory decoding off ({how})",
            plat.name, plat.display_ver
        );
    }
    let Some(mut mmio) = Mmio::new(bar0, size) else {
        return format!(
            "{} display {}: not driven, BAR0 too small",
            plat.name, plat.display_ver
        );
    };
    let pipes = match readout::read_all(&mut mmio, &plat) {
        Ok(r) => {
            let v: alloc::vec::Vec<String> = r
                .pipes
                .iter()
                .map(|a| {
                    format!(
                        "pipe {} -> port {} {}x{}",
                        a.pipe, a.port, a.timing.hdisplay, a.timing.vdisplay
                    )
                })
                .collect();
            if v.is_empty() {
                String::from("no active pipe")
            } else {
                v.join(", ")
            }
        }
        Err(e) => format!("readout: {e}"),
    };
    format!(
        "{} display {}: not driven, read only ({pipes}); {how}",
        plat.name, plat.display_ver
    )
}

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "intel-display"
    }

    fn matches(&self, d: &PciDev) -> bool {
        d.vendor == 0x8086 && d.class == 0x03 && platform::lookup(d.device).is_ok()
    }

    fn probe(&self, d: &PciDev) -> Result<()> {
        let plat = platform::lookup(d.device).map_err(|e| format!("{e}"))?;
        let bar0 = d.bars[0].filter(|b| !b.io).ok_or("BAR0 missing")?;
        if !optin::allowed(d.device, OPTED_IN.load(Ordering::Relaxed)) {
            return Err(read_only(d, plat, bar0.addr, bar0.size));
        }
        if DEVICE.lock().is_some() {
            return Err(String::from("an Intel display device is bound already"));
        }
        d.enable();
        let mmio = Mmio::new(bar0.addr, bar0.size).ok_or("BAR0 smaller than 16 MiB")?;
        let aperture = d.bars[2]
            .filter(|b| !b.io && b.size > 0)
            .map(|b| (b.addr, b.size));
        let vbt = opregion::vbt(d);
        let mut dev = Device::takeover(mmio, plat, (d.bus, d.dev, d.func), vbt, aperture)
            .map_err(|e| format!("{e}"))?;
        dev.collect_modes();
        // the GT with GMD_ID and GuC submission came with display 14 (Meteor Lake)
        if plat.display_ver >= 14 {
            gt::probe(&mut dev.mmio);
        }
        crate::kprintln!(
            "[intel-display] {} display {}: {}",
            plat.name,
            plat.display_ver,
            summary(&dev)
        );
        *DEVICE.lock() = Some(dev);
        crate::grafik::anmelden(Box::new(anzeige::IntelAnzeige));
        Ok(())
    }
}

fn summary(dev: &Device) -> String {
    let active: alloc::vec::Vec<String> = dev
        .boot
        .found
        .iter()
        .map(|a| {
            format!(
                "pipe {} -> port {} {}x{}",
                a.pipe, a.port, a.timing.hdisplay, a.timing.vdisplay
            )
        })
        .collect();
    format!(
        "{} output(s){}, {}, {} mode(s)",
        dev.outputs.len(),
        if dev.outputs_from_vbt {
            " from VBT"
        } else {
            ""
        },
        if active.is_empty() {
            String::from("no active pipe")
        } else {
            active.join(", ")
        },
        dev.modes.len()
    )
}

/// One line for `/proc/ultar/geraet` style reports. Does not wait while a mode set holds the
/// device (that can take a second).
pub fn text() -> Option<String> {
    match DEVICE.try_lock() {
        Some(g) => g.as_ref().map(summary),
        None => Some(String::from("busy (mode set running)")),
    }
}
