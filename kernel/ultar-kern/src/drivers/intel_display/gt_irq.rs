//! Interrupts of the GT in operation (`ultar_intel_gt::irq`): the device sends a message
//! signalled interrupt (MSI) to the core that set it up when a job ends or the GuC has a message;
//! the handler acknowledges it and wakes the threads that wait for the GPU. Without it they were
//! woken by a timer every 250 µs, which costs power the GT then lacks.

use super::device::Device;
use super::mmio::Mmio;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;
use ultar_intel_gt::irq;

/// The interrupt vector of the GT.
pub const VECTOR: u8 = crate::trap::VEC_GPU as u8;
const PCI_CAP_MSI: u8 = 0x05;
/// Local APIC message address (Intel SDM vol. 3 §11.11.1): physical destination in bits 19:12.
const MSI_ADDRESS: u32 = 0xfee0_0000;

/// Register access for the handler: its own handle on BAR0, used for interrupt registers only.
static REGS: Mutex<Option<Mmio>> = Mutex::new(None);
static ON: AtomicBool = AtomicBool::new(false);
/// Interrupts taken, engine events, GuC events, sources without identity.
static COUNTS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

/// Whether interrupts arrive (set up and at least one taken).
pub fn working() -> bool {
    ON.load(Ordering::Relaxed) && COUNTS[0].load(Ordering::Relaxed) > 0
}

/// Counters as text for the log.
pub fn counts() -> [u64; 4] {
    core::array::from_fn(|i| COUNTS[i].load(Ordering::Relaxed))
}

fn msi_cap(pci: (u8, u8, u8)) -> Option<u8> {
    let (b, d, f) = pci;
    if crate::pci::read16(b, d, f, 0x06) & 0x10 == 0 {
        return None;
    }
    let mut p = crate::pci::read8(b, d, f, 0x34) & 0xfc;
    for _ in 0..48 {
        if p == 0 {
            return None;
        }
        if crate::pci::read8(b, d, f, p) == PCI_CAP_MSI {
            return Some(p);
        }
        p = crate::pci::read8(b, d, f, p + 1) & 0xfc;
    }
    None
}

/// Points the device's MSI at this core and opens the GT's interrupt registers.
pub fn setup(dev: &mut Device) -> Result<(), &'static str> {
    let (b, d, f) = dev.pci;
    let cap = msi_cap(dev.pci).ok_or("no MSI capability")?;
    let head = crate::pci::read32(b, d, f, cap);
    let control = (head >> 16) as u16;
    let wide = control & 1 << 7 != 0;
    let dest = crate::apic::id() & 0xff;
    crate::pci::write32(b, d, f, cap + 4, MSI_ADDRESS | dest << 12);
    let data_at = if wide {
        crate::pci::write32(b, d, f, cap + 8, 0);
        cap + 12
    } else {
        cap + 8
    };
    // edge, fixed delivery
    crate::pci::write32(b, d, f, data_at, VECTOR as u32);
    if control & 1 << 8 != 0 {
        // per-vector mask: let vector 0 through
        crate::pci::write32(b, d, f, data_at + 4, 0);
    }
    // one vector (multiple message enable 0), MSI on
    let control = (control & !(7 << 4)) | 1;
    crate::pci::write32(b, d, f, cap, (head & 0xffff) | (control as u32) << 16);
    *REGS.lock() = Some(dev.mmio.alias());
    irq::enable(&mut dev.mmio);
    ON.store(true, Ordering::Relaxed);
    Ok(())
}

/// The interrupt handler (interrupts are off).
pub fn on_interrupt() {
    let Some(mut guard) = REGS.try_lock() else {
        return;
    };
    let Some(mmio) = guard.as_mut() else { return };
    let ev = irq::handle(mmio);
    drop(guard);
    COUNTS[0].fetch_add(1, Ordering::Relaxed);
    COUNTS[1].fetch_add(u64::from(ev.engines != 0), Ordering::Relaxed);
    COUNTS[2].fetch_add(u64::from(ev.guc), Ordering::Relaxed);
    COUNTS[3].fetch_add(u64::from(ev.lost), Ordering::Relaxed);
    if ev.engines != 0 || ev.guc {
        crate::apic::wake_waiters();
    }
}
