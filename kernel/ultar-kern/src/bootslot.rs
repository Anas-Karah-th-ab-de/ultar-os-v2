//! The slot this kernel was started from (A/B start chain, see `kernel/ultar-loader`).
//!
//! The loader passes `ultar.slot=a|b`, `ultar.trial=1` for a counted attempt, the GPT GUID of
//! the disk it read the slot table from, and the time it spent, as UEFI load options. Without
//! them the kernel was started directly (installation medium, installation from before A/B):
//! it then mounts its root the old way. Load options with a malformed `ultar.` word are
//! refused as a whole: the kernel then mounts no system partition at all rather than guess.

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use spin::Mutex;
use ultar_slots::layout::BootOptions;
use ultar_slots::Slot;

const NONE: u8 = 0xFF;
static SLOT: AtomicU8 = AtomicU8::new(NONE);
static TRIAL: AtomicBool = AtomicBool::new(false);
static REFUSED: AtomicBool = AtomicBool::new(false);
static LOADER_TICKS: AtomicU64 = AtomicU64::new(0);
static DISK: Mutex<Option<[u8; 16]>> = Mutex::new(None);

/// Takes the load options of this image (UTF-16 units, as the firmware passed them).
pub fn init(options: &[u16]) {
    match BootOptions::parse(options) {
        Ok(o) => {
            // a slot without the disk it lives on is not usable: the partitions would be guessed
            if let (Some(slot), Some(disk)) = (o.slot, o.disk) {
                SLOT.store(slot.index() as u8, Ordering::Relaxed);
                *DISK.lock() = Some(disk);
            } else if o.slot.is_some() {
                REFUSED.store(true, Ordering::Relaxed);
            }
            TRIAL.store(o.trial && o.slot.is_some(), Ordering::Relaxed);
            LOADER_TICKS.store(o.loader_ticks.unwrap_or(0), Ordering::Relaxed);
        }
        Err(_) => REFUSED.store(true, Ordering::Relaxed),
    }
}

/// The running slot, if the A/B loader started this kernel.
pub fn slot() -> Option<Slot> {
    Slot::from_index(SLOT.load(Ordering::Relaxed))
}

/// Whether this start is a counted attempt of a new slot.
pub fn trial() -> bool {
    TRIAL.load(Ordering::Relaxed)
}

/// Whether the loader's options were malformed (no system partition is mounted then).
pub fn refused() -> bool {
    REFUSED.load(Ordering::Relaxed)
}

/// GPT GUID of the disk the system was started from.
pub fn disk() -> Option<[u8; 16]> {
    *DISK.lock()
}

/// `/proc/ultar/boot_slot`: `a`, `b`, or `-` when not started through the A/B loader.
pub fn proc_text() -> alloc::string::String {
    alloc::format!("{}\n", slot().map_or("-", |s| s.letter()))
}

/// One log line after the clock is calibrated: slot, attempt, time the loader took.
pub fn report() {
    let Some(slot) = slot() else {
        if refused() {
            crate::kprintln!("[boot] loader options refused - no system partition is mounted");
        }
        return;
    };
    let khz = crate::arch::tsc_khz().max(1);
    let micros = LOADER_TICKS.load(Ordering::Relaxed).saturating_mul(1000) / khz;
    crate::kprintln!("[boot] slot {}{}, loader {} us", slot.letter(), if trial() { " (trial)" } else { "" }, micros);
}

/// The start cannot go on (root missing, kernel panic). A counted attempt restarts the machine
/// at once, so the loader spends the next attempt or falls back; a regular start keeps the
/// console for repair.
pub fn start_failed(reason: &str) {
    crate::kprintln!("[boot] start failed: {reason}");
    if trial() {
        crate::kprintln!("[boot] trial start of slot {} failed - restarting", slot().map_or("-", |s| s.letter()));
        // the kernel's own restart (ACPI reset register, then the keyboard controller): the
        // firmware's runtime services are not safe to call from a panic
        crate::power::neustart()
    }
}
