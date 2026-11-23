//! UEFI entry of the Ultar loader: binds the decision logic of the library to the firmware.
//!
//! Flow: own start partition → its whole disk (Block I/O) → partitions by GPT name →
//! [`ultar_loader::boot::run`] (decision written and flushed before every start) → kernel image
//! from `ultar-kernel-<slot>` → `LoadImage` from memory → load options → `StartImage`.
//! Rescue: `\EFI\ULTAR\RESCUE.EFI` on the start partition if present, otherwise a message and
//! a return to the firmware, which goes on with its next boot option (installation medium).
#![no_std]
#![no_main]
#![deny(clippy::unwrap_used, clippy::expect_used)]

mod blockio;
mod uefi;

use blockio::{path_bytes, BootDisk};
use uefi::{Handle, LoadedImage, Pages, Status, SystemTable, LOADED_IMAGE, LOAD_ERROR, NOT_FOUND};
use ultar_loader::boot::{self, Start};
use ultar_loader::disk::{self, Disk, Parts, MAX_BLOCK};
use ultar_loader::{msg, pe};
use ultar_slots::layout::{BootOptions, KERNEL_IMAGE_MAX_BYTES, OPTIONS_MAX};
use ultar_slots::Slot;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    uefi::reset()
}

/// Processor time stamp (x86_64: TSC; aarch64: virtual counter).
#[cfg(target_arch = "x86_64")]
fn ticks() -> u64 {
    // SAFETY: RDTSC has no preconditions and touches no memory.
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// Processor time stamp (x86_64: TSC; aarch64: virtual counter).
#[cfg(target_arch = "aarch64")]
fn ticks() -> u64 {
    let value: u64;
    // SAFETY: reading CNTVCT_EL0 is allowed at EL1/EL2 under UEFI and touches no memory.
    unsafe { core::arch::asm!("mrs {}, cntvct_el0", out(reg) value) };
    value
}

/// Firmware device path = `base` + a file path node `file` + end node, in `out`.
fn file_path<'a>(base: &[u8], file: &str, out: &'a mut [u8; 1024]) -> Option<&'a [u8]> {
    let units = file.encode_utf16().count() + 1;
    let node = 4 + 2 * units;
    let end = base.len() + node;
    if end + 4 > out.len() {
        return None;
    }
    out[..base.len()].copy_from_slice(base);
    out[base.len()..base.len() + 4].copy_from_slice(&[4, 4, node as u8, (node >> 8) as u8]);
    for (i, unit) in file.encode_utf16().chain(core::iter::once(0)).enumerate() {
        let at = base.len() + 4 + 2 * i;
        out[at..at + 2].copy_from_slice(&unit.to_le_bytes());
    }
    out[end..end + 4].copy_from_slice(&[0x7F, 0xFF, 4, 0]);
    Some(&out[..end + 4])
}

struct Ctx {
    image: Handle,
    esp: Handle,
    entry_ticks: u64,
    parts: Parts,
    scratch: Pages,
}

/// Reads the kernel image of `slot` into fresh pages; returns them and the image length.
fn read_kernel(ctx: &mut Ctx, disk: &mut BootDisk, slot: Slot) -> Option<(Pages, usize)> {
    let entry = ctx.parts.kernel(slot)?;
    let bs = disk.block_size();
    let part_bytes = entry.blocks().checked_mul(bs as u64)?;
    let head_len = (pe::HEADER_READ as u64).min(part_bytes) as usize / bs * bs;
    let head = ctx.scratch.bytes().get_mut(..head_len)?;
    disk.read(entry.first_lba, head).ok()?;
    let len = pe::image_len(head, pe::MACHINE).ok()?;
    if len > part_bytes || len > KERNEL_IMAGE_MAX_BYTES {
        return None;
    }
    let len = len as usize;
    let read_len = len.div_ceil(bs) * bs;
    let mut pages = Pages::new(read_len)?;
    disk.read(entry.first_lba, pages.bytes().get_mut(..read_len)?)
        .ok()?;
    Some((pages, len))
}

/// Loads and starts the kernel of `start.slot`; returns only when that failed.
fn start_slot(ctx: &mut Ctx, disk: &mut BootDisk, start: Start) {
    let Some((mut pages, len)) = read_kernel(ctx, disk, start.slot) else {
        return;
    };
    let mut buf = [0u8; 1024];
    let name = if start.slot == Slot::A {
        "\\EFI\\ULTAR\\KERNEL-A.EFI"
    } else {
        "\\EFI\\ULTAR\\KERNEL-B.EFI"
    };
    let Some(path) = path_bytes(ctx.esp).and_then(|base| file_path(base, name, &mut buf)) else {
        return;
    };
    let Ok(child) = uefi::load_image(ctx.image, path.as_ptr(), &pages.bytes()[..len]) else {
        return;
    };
    drop(pages);
    let child_image = uefi::protocol(child, &LOADED_IMAGE) as *mut LoadedImage;
    let Some(mut options) = Pages::new(2 * OPTIONS_MAX) else {
        return;
    };
    let mut units = [0u16; OPTIONS_MAX];
    let opts = BootOptions {
        slot: Some(start.slot),
        trial: start.trial,
        loader_ticks: Some(ticks().wrapping_sub(ctx.entry_ticks)),
        disk: Some(ctx.parts.disk_guid),
    };
    let (Some(n), false) = (opts.encode(&mut units), child_image.is_null()) else {
        return;
    };
    for (i, u) in units[..n].iter().enumerate() {
        options.bytes()[2 * i..2 * i + 2].copy_from_slice(&u.to_le_bytes());
    }
    // SAFETY: `child_image` is the loaded image protocol of the image just loaded; UEFI lets
    // the parent set the load options before StartImage. The pages stay allocated (forgotten
    // below) for as long as the kernel may read them.
    unsafe {
        (*child_image).options_size = (2 * n) as u32;
        (*child_image).options = options.bytes().as_ptr().cast();
    }
    core::mem::forget(options);
    // A counted attempt that hangs in the firmware phase is restarted by the firmware watchdog;
    // the kernel disarms it when it takes over the machine.
    if start.trial {
        uefi::watchdog(300);
    }
    let _ = uefi::start_image(child);
    uefi::watchdog(0);
    uefi::unload_image(child);
}

fn rescue(image: Handle, esp: Handle, text: &str) -> Status {
    let mut buf = [0u8; 1024];
    if let Some(path) =
        path_bytes(esp).and_then(|base| file_path(base, "\\EFI\\ULTAR\\RESCUE.EFI", &mut buf))
    {
        if let Ok(child) = uefi::load_image(image, path.as_ptr(), &[]) {
            uefi::print(msg::RESCUE);
            return uefi::start_image(child);
        }
    }
    uefi::print(text);
    uefi::stall(5_000_000);
    NOT_FOUND
}

/// Firmware entry point.
#[no_mangle]
pub extern "efiapi" fn efi_main(image: Handle, st: *mut SystemTable) -> Status {
    let entry_ticks = ticks();
    uefi::init(st);
    let own = uefi::protocol(image, &LOADED_IMAGE) as *const LoadedImage;
    if own.is_null() {
        return LOAD_ERROR;
    }
    // SAFETY: the firmware's loaded image protocol of this program.
    let esp = unsafe { (*own).device };
    let (Some(mut scratch), Some(mut block)) = (Pages::new(64 * 1024), Pages::new(MAX_BLOCK))
    else {
        return LOAD_ERROR;
    };
    let Some(mut disk) = BootDisk::of_partition(esp) else {
        return rescue(image, esp, msg::NO_LAYOUT);
    };
    let parts = match disk::find_parts(&mut disk, scratch.bytes()) {
        Ok(p) => p,
        Err(_) => return rescue(image, esp, msg::NO_LAYOUT),
    };
    let mut ctx = Ctx {
        image,
        esp,
        entry_ticks,
        parts,
        scratch,
    };
    let _why = boot::run(&mut disk, &parts, block.bytes(), &mut |d, s| {
        start_slot(&mut ctx, d, s)
    });
    rescue(image, esp, msg::NO_SLOT)
}
