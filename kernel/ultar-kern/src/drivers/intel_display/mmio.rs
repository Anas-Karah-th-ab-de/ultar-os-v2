//! Register access of the Intel display driver: volatile 32-bit loads and stores into BAR0 (the
//! "GTTMMADR" BAR: registers in the lower 8 MiB, the GGTT in the upper 8 MiB).
//!
//! Caching: the PCI scan asks for an uncached identity mapping of the BAR, but `paging::init`
//! already mapped `0..max(top of RAM, 4 GiB)` write-back and `map_identity` keeps entries that
//! exist. A BAR below 4 GiB is therefore mapped WB in the page tables and is uncached only
//! because the firmware's MTRRs mark the MMIO hole UC (PAT WB + MTRR UC = UC, Intel SDM vol. 3
//! §12.5.2.2). That holds on every PC firmware the driver was run on; a kernel-wide remap of
//! device ranges is the open fix (docs/integration/intel-display.md §0).

use ultar_intel_display::ggtt::GGTT_OFFSET;
use ultar_intel_display::Hw;

/// Size of BAR0 on display 9 and 14 (16 MiB).
pub const BAR0_SIZE: u64 = 16 << 20;

/// BAR0 of one device.
pub struct Mmio {
    base: u64,
}

impl Mmio {
    /// `base` must be the identity-mapped BAR0 of an Intel graphics device of at least
    /// [`BAR0_SIZE`] bytes; [`Mmio::new`] checks size and alignment.
    pub fn new(base: u64, size: u64) -> Option<Mmio> {
        (base != 0 && size >= BAR0_SIZE && base % 4096 == 0).then_some(Mmio { base })
    }

    /// A second handle on the same BAR0, for the interrupt handler (it touches only the
    /// interrupt registers, which nothing else uses).
    pub fn alias(&self) -> Mmio {
        Mmio { base: self.base }
    }

    fn store(&mut self, reg: u32, val: u32) {
        // SAFETY: callers pass a 4-byte aligned offset inside the 16 MiB BAR0 (checked by
        // `write`/`write_gtt`), which is identity-mapped and uncached through the MTRRs (module
        // comment); a volatile 32-bit store is the access the hardware expects.
        unsafe { core::ptr::write_volatile((self.base + reg as u64) as *mut u32, val) }
    }
}

impl Hw for Mmio {
    fn read(&mut self, reg: u32) -> u32 {
        if reg as u64 >= BAR0_SIZE || reg % 4 != 0 {
            return u32::MAX;
        }
        // SAFETY: `reg` is a 4-byte aligned offset inside the 16 MiB BAR0, identity-mapped and
        // uncached through the MTRRs (module comment); a volatile 32-bit load has no effect on
        // memory the kernel owns.
        unsafe { core::ptr::read_volatile((self.base + reg as u64) as *const u32) }
    }

    /// Registers only: the GGTT half of BAR0 is refused (written through `write_gtt` by
    /// `ultar_intel_display::ggtt` alone).
    fn write(&mut self, reg: u32, val: u32) {
        if reg >= GGTT_OFFSET || reg % 4 != 0 {
            crate::kprintln!(
                "[intel-display] write to {reg:#x} refused (outside the register half of BAR0)"
            );
            return;
        }
        self.store(reg, val);
    }

    fn write_gtt(&mut self, reg: u32, val: u32) {
        if reg < GGTT_OFFSET || reg as u64 >= BAR0_SIZE || reg % 4 != 0 {
            crate::kprintln!("[intel-display] GGTT write to {reg:#x} refused (outside the table)");
            return;
        }
        self.store(reg, val);
    }

    fn delay_us(&mut self, us: u32) {
        crate::arch::sleep_us(us as u64);
    }

    fn now_us(&mut self) -> u64 {
        crate::arch::uptime_ns() / 1000
    }
}
