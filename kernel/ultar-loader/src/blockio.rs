//! The boot disk through `EFI_BLOCK_IO_PROTOCOL`, and finding it from the loader's own
//! start partition.

use crate::uefi::{self, Handle, Status, BLOCK_IO, DEVICE_PATH};
use core::ffi::c_void;
use ultar_loader::disk::{Disk, DiskError};

#[repr(C)]
struct Media {
    media_id: u32,
    removable: u8,
    present: u8,
    logical_partition: u8,
    read_only: u8,
    write_caching: u8,
    block_size: u32,
    io_align: u32,
    last_block: u64,
}

#[repr(C)]
struct BlockIoProto {
    revision: u64,
    media: *const Media,
    reset: usize,
    read_blocks: extern "efiapi" fn(*mut BlockIoProto, u32, u64, usize, *mut c_void) -> Status,
    write_blocks: extern "efiapi" fn(*mut BlockIoProto, u32, u64, usize, *const c_void) -> Status,
    flush_blocks: extern "efiapi" fn(*mut BlockIoProto) -> Status,
}

/// The whole disk the loader was started from.
pub struct BootDisk {
    proto: *mut BlockIoProto,
    media_id: u32,
    block_size: usize,
    last_block: u64,
}

/// Longest device path the loader handles.
const PATH_MAX: usize = 4096;

/// Length of a device path without its end node, and the length up to its first media node
/// (type 4: hard drive, file …) – the part that names the disk.
fn path_lengths(path: *const u8) -> Option<(usize, usize)> {
    if path.is_null() {
        return None;
    }
    let (mut len, mut disk) = (0usize, None);
    loop {
        // SAFETY: firmware device paths are sequences of nodes (type, subtype, u16 length)
        // ending in an end node (0x7F, 0xFF); each read stays inside the current node header
        // and the walk is bounded by PATH_MAX.
        let (kind, sub, node) = unsafe {
            (
                *path.add(len),
                *path.add(len + 1),
                u16::from_le_bytes([*path.add(len + 2), *path.add(len + 3)]) as usize,
            )
        };
        if kind == 0x7F && sub == 0xFF {
            return Some((len, disk.unwrap_or(len)));
        }
        if kind == 4 && disk.is_none() {
            disk = Some(len);
        }
        if node < 4 || len + node > PATH_MAX {
            return None;
        }
        len += node;
    }
}

/// Bytes of a device path (not including the end node).
pub fn path_bytes(handle: Handle) -> Option<&'static [u8]> {
    let path = uefi::protocol(handle, &DEVICE_PATH) as *const u8;
    let (len, _) = path_lengths(path)?;
    // SAFETY: `path_lengths` walked exactly these bytes of the firmware's device path.
    Some(unsafe { core::slice::from_raw_parts(path, len) })
}

impl BootDisk {
    /// The whole-disk Block I/O handle whose device path is the disk part of `partition`'s.
    pub fn of_partition(partition: Handle) -> Option<Self> {
        let part_path = uefi::protocol(partition, &DEVICE_PATH) as *const u8;
        let (_, disk_len) = path_lengths(part_path)?;
        // SAFETY: inside the walked path (see path_lengths).
        let disk_path = unsafe { core::slice::from_raw_parts(part_path, disk_len) };
        for &h in uefi::handles(&BLOCK_IO) {
            let proto = uefi::protocol(h, &BLOCK_IO) as *mut BlockIoProto;
            if proto.is_null() || path_bytes(h) != Some(disk_path) {
                continue;
            }
            // SAFETY: a Block I/O protocol instance always points to its media description.
            let m = unsafe { &*(*proto).media };
            if m.logical_partition != 0 || m.present == 0 || m.io_align > 4096 {
                continue;
            }
            return Some(Self {
                proto,
                media_id: m.media_id,
                block_size: m.block_size as usize,
                last_block: m.last_block,
            });
        }
        None
    }
}

impl Disk for BootDisk {
    fn block_size(&self) -> usize {
        self.block_size
    }

    fn last_block(&self) -> u64 {
        self.last_block
    }

    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), DiskError> {
        // SAFETY: ReadBlocks per UEFI 2.10 §13.9 into a caller buffer of exactly buf.len()
        // bytes; the caller's buffers are page aligned (≥ IoAlign, checked above).
        let st = unsafe {
            ((*self.proto).read_blocks)(
                self.proto,
                self.media_id,
                lba,
                buf.len(),
                buf.as_mut_ptr().cast(),
            )
        };
        (st == 0).then_some(()).ok_or(DiskError)
    }

    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), DiskError> {
        // SAFETY: WriteBlocks per UEFI 2.10 §13.9, buffer as for read.
        let st = unsafe {
            ((*self.proto).write_blocks)(
                self.proto,
                self.media_id,
                lba,
                buf.len(),
                buf.as_ptr().cast(),
            )
        };
        (st == 0).then_some(()).ok_or(DiskError)
    }

    fn flush(&mut self) -> Result<(), DiskError> {
        // SAFETY: FlushBlocks per UEFI 2.10 §13.9.
        let st = unsafe { ((*self.proto).flush_blocks)(self.proto) };
        (st == 0).then_some(()).ok_or(DiskError)
    }
}
