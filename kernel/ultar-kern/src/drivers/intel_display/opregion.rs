//! The Video BIOS Table from the ACPI OpRegion of the graphics device: the firmware puts the
//! OpRegion's physical address into PCI config register 0xFC (ASLS); the VBT is in mailbox 4 or,
//! when it is larger, at the "raw VBT" address of mailbox 3 (OpRegion 2.0+).

use crate::pci::PciDev;
use alloc::vec::Vec;

const SIGNATURE: &[u8; 16] = b"IntelGraphicsMem";
const OPREGION_SIZE: u64 = 8 * 1024;
const MBOX4: u64 = 0x400;
const MBOX4_SIZE: usize = 0x1800;
const RVDA: u64 = 0x3ba;
const RVDS: u64 = 0x3c2;
const VERSION: u64 = 0x16;
/// Highest physical address read here (46-bit physical address width).
const PHYS_LIMIT: u64 = 1 << 46;

/// Reads `len` bytes of firmware memory at physical `addr`; `None` when the range is not a
/// plausible physical range (overflow, beyond the physical address width).
fn read_bytes(addr: u64, len: usize) -> Option<Vec<u8>> {
    let end = addr
        .checked_add(len as u64)
        .filter(|e| *e <= PHYS_LIMIT && len > 0)?;
    crate::paging::map_identity(addr & !0xfff, end.next_multiple_of(0x1000), false);
    let mut v = alloc::vec![0u8; len];
    for (i, b) in v.iter_mut().enumerate() {
        // SAFETY: `[addr, end)` was identity-mapped just above and lies below the physical
        // address limit; it is firmware memory (ACPI NVS) that nothing else writes while the
        // kernel runs; byte reads have no side effects.
        *b = unsafe { core::ptr::read_volatile((addr + i as u64) as *const u8) };
    }
    Some(v)
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4)
        .map_or(0, |x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

/// The VBT image of `dev`, if the firmware provides one.
pub fn vbt(dev: &PciDev) -> Option<Vec<u8>> {
    let asls = dev.r32(0xfc) as u64;
    if asls == 0 || asls == 0xffff_ffff {
        return None;
    }
    let head = read_bytes(asls, OPREGION_SIZE as usize)?;
    if head.get(..16)? != SIGNATURE {
        return None;
    }
    let mbox4 = head.get(MBOX4 as usize..MBOX4 as usize + MBOX4_SIZE)?;
    if mbox4.get(..4)? == b"$VBT" {
        return Some(mbox4.to_vec());
    }
    let ver = u16::from_le_bytes([head[VERSION as usize], head[VERSION as usize + 1]]);
    let rvda =
        u32_at(&head, RVDA as usize) as u64 | (u32_at(&head, RVDA as usize + 4) as u64) << 32;
    let rvds = u32_at(&head, RVDS as usize);
    // OpRegion 2.1+: relative to the OpRegion, 2.0: absolute; bounded near the OpRegion
    let (addr, len) = ultar_intel_display::vbt::raw_vbt_range(asls, ver, rvda, rvds)?;
    let img = read_bytes(addr, len)?;
    (img.get(..4)? == b"$VBT").then_some(img)
}
