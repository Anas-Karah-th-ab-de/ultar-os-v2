//! Intel NPU driver of the Ultar kernel ("AI Boost", NPU 3720 of Meteor Lake / Arrow Lake, e.g.
//! 8086:7d1d in the Beelink GTi15 Ultra).
//!
//! Stage N0: identification, a read-out of the buttress (clock fuses, power state) and – in the
//! self test (NPUTEST) – a full power cycle: clock on, IP block out of reset, power island and
//! interconnects up, hardware version read, power down again. The logic lives in
//! `drivers/kernel/intel-npu` (pure, tested on the host); this module adds MMIO and the kernel
//! interfaces. Plan and state: `docs/integration/intel-npu.md`.
//!
//! Opt-in: the driver writes to the hardware only when the machine opted in (boot option
//! `ultar.intel_npu=on`, the file `\ultar\intel-npu` with `on` on the start partition, or the
//! device id in `optin::ALLOWLIST`, empty for now). Otherwise it only reads the buttress.

use crate::driver::{Driver, Result};
use crate::kprintln;
use crate::pci::PciDev;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use ultar_intel_npu::ident::{self, Model};
use ultar_intel_npu::pll::Ratios;
use ultar_intel_npu::{optin, power, state, Hw, Reg, Space};

/// The driver.
pub struct Drv;
/// The driver instance for `drivers::ALL`.
pub static DRV: Drv = Drv;

/// Set when the boot option or the opt-in file was found (firmware phase).
static OPTED_IN: AtomicBool = AtomicBool::new(false);

/// The bound device (one NPU per machine).
static DEVICE: Mutex<Option<Npu>> = Mutex::new(None);

/// Takes `ultar.intel_npu=on` out of the first `n` UEFI load option units (the A/B start chain
/// refuses unknown `ultar.` words) and returns the remaining length.
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

/// The two register blocks of one NPU.
struct Mmio {
    ip: (u64, u64),
    buttress: (u64, u64),
}

impl Mmio {
    /// Address of a register, if it is a 4-byte aligned offset inside its block.
    fn addr(&self, reg: Reg) -> Option<u64> {
        let (base, size) = match reg.0 {
            Space::Ip => self.ip,
            Space::Buttress => self.buttress,
        };
        (reg.1 % 4 == 0 && (reg.1 as u64) + 4 <= size).then_some(base + reg.1 as u64)
    }
}

impl Hw for Mmio {
    fn read(&mut self, reg: Reg) -> u32 {
        let Some(a) = self.addr(reg) else {
            return u32::MAX;
        };
        // SAFETY: `a` is a 4-byte aligned address inside a memory BAR of the NPU, which the PCI
        // scan identity-mapped as device memory; a volatile 32-bit load is the access the
        // hardware expects and has no effect on memory the kernel owns.
        unsafe { core::ptr::read_volatile(a as *const u32) }
    }

    fn write(&mut self, reg: Reg, val: u32) {
        let Some(a) = self.addr(reg) else {
            kprintln!("[intel-npu] write to {reg} refused (outside the block)");
            return;
        };
        // SAFETY: as in `read`; a volatile 32-bit store into the device's own register block.
        unsafe { core::ptr::write_volatile(a as *mut u32, val) }
    }

    fn delay_us(&mut self, us: u32) {
        crate::arch::sleep_us(us as u64);
    }

    fn now_us(&mut self) -> u64 {
        crate::arch::uptime_ns() / 1000
    }
}

/// One NPU.
struct Npu {
    mmio: Mmio,
    model: Model,
    ratios: Ratios,
}

fn blocks(d: &PciDev) -> core::result::Result<Mmio, &'static str> {
    let bar = |i: usize, what| {
        d.bars[i]
            .filter(|b| !b.io && b.addr != 0 && b.addr % 4096 == 0 && b.size >= 4096)
            .ok_or(what)
    };
    let ip = bar(0, "BAR0 (IP block) missing")?;
    let buttress = bar(4, "BAR4 (buttress) missing")?;
    Ok(Mmio {
        ip: (ip.addr, ip.size),
        buttress: (buttress.addr, buttress.size),
    })
}

impl Driver for Drv {
    fn name(&self) -> &'static str {
        "intel-npu"
    }

    fn matches(&self, d: &PciDev) -> bool {
        d.vendor == ident::VENDOR && ident::known(d.device)
    }

    fn probe(&self, d: &PciDev) -> Result<()> {
        let model = ident::lookup(d.device).map_err(|e| format!("{e}"))?;
        let mut mmio = blocks(d)?;
        if !optin::allowed(d.device, OPTED_IN.load(Ordering::Relaxed)) {
            let how =
                "opt in with the boot option ultar.intel_npu=on or the file \\ultar\\intel-npu";
            // without opt-in: no PCI command write, no register write
            let seen = if d.r32(0x04) & 0x2 == 0 {
                String::from("memory decoding off")
            } else {
                match state::read(&mut mmio) {
                    Ok(f) => format!("{f}"),
                    Err(e) => format!("{e}"),
                }
            };
            return Err(format!(
                "{}: not driven, read only ({seen}); {how}",
                model.name
            ));
        }
        if DEVICE.lock().is_some() {
            return Err(String::from("an Intel NPU is bound already"));
        }
        d.enable();
        let facts = state::read(&mut mmio).map_err(|e| format!("{e}"))?;
        kprintln!("[intel-npu] {}: {facts}", model.name);
        *DEVICE.lock() = Some(Npu {
            mmio,
            model,
            ratios: facts.ratios,
        });
        Ok(())
    }
}

/// One line for reports.
pub fn text() -> Option<String> {
    let mut g = DEVICE.lock();
    let npu = g.as_mut()?;
    Some(match state::read(&mut npu.mmio) {
        Ok(f) => format!("{}: {f}", npu.model.name),
        Err(e) => format!("{}: {e}", npu.model.name),
    })
}

/// Failed checks, collected.
struct Report {
    fails: Vec<&'static str>,
}

impl Report {
    fn check(&mut self, ok: bool, what: &'static str, detail: &str) {
        kprintln!(
            "NPUTEST: {} {what}{}{detail}",
            if ok { "ok  " } else { "FAIL" },
            if detail.is_empty() { "" } else { ": " }
        );
        if !ok {
            self.fails.push(what);
        }
    }
}

fn run_on(npu: &mut Npu, r: &mut Report) {
    match state::read(&mut npu.mmio) {
        Ok(f) => r.check(
            f.ratios.pn != 0 && f.ratios.max >= f.ratios.min,
            "clock fuses",
            &format!("{f}"),
        ),
        Err(e) => return r.check(false, "clock fuses", &format!("{e}")),
    }
    let t0 = crate::arch::uptime_ns();
    let up = power::up(&mut npu.mmio, &npu.ratios);
    let us = (crate::arch::uptime_ns() - t0) / 1000;
    match up {
        Ok(rep) => {
            r.check(true, "power-up", &format!("{us} µs"));
            if rep.wp0_failed || rep.d0i3_failed {
                kprintln!(
                    "NPUTEST: note: clock-off before power-up failed: {}, leaving D0i3 failed: {}",
                    rep.wp0_failed,
                    rep.d0i3_failed
                );
            }
            match power::hw_version(&mut npu.mmio) {
                Some(v) => r.check(
                    v.sku() == npu.model.sku,
                    "IP block answers with the expected hardware version",
                    &format!("{:#06x} rev {}", v.sku(), v.revision),
                ),
                None => r.check(
                    false,
                    "IP block answers with the expected hardware version",
                    "all ones",
                ),
            }
            match state::read(&mut npu.mmio) {
                Ok(f) => r.check(
                    f.ready && f.pll_locked && f.current_ratio != 0,
                    "clock locked, NPU ready",
                    &format!("{f}"),
                ),
                Err(e) => r.check(false, "clock locked, NPU ready", &format!("{e}")),
            }
        }
        Err(e) => r.check(false, "power-up", &format!("{e}")),
    }
    // down in every case: a half powered NPU must not stay behind
    match power::down(&mut npu.mmio, &npu.ratios) {
        Ok(()) => match state::read(&mut npu.mmio) {
            Ok(f) => r.check(!f.ready, "power-down", &format!("{f}")),
            Err(e) => r.check(false, "power-down", &format!("{e}")),
        },
        Err(e) => r.check(false, "power-down", &format!("{e}")),
    }
}

/// NPUTEST – the driver's self test on real hardware. Ends with `NPUTEST: ALLES OK` or
/// `NPUTEST: FEHLER (<list>)`.
pub fn selftest() {
    let mut guard = DEVICE.lock();
    let Some(npu) = guard.as_mut() else {
        kprintln!("NPUTEST: no Intel NPU bound");
        return;
    };
    let mut r = Report { fails: Vec::new() };
    run_on(npu, &mut r);
    if r.fails.is_empty() {
        kprintln!("NPUTEST: ALLES OK");
    } else {
        kprintln!("NPUTEST: FEHLER ({})", r.fails.join("; "));
    }
}
