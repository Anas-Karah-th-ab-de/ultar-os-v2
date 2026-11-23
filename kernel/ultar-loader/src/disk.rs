//! The disk as the loader sees it, and the partitions it needs from it.

use ultar_slots::gpt::{Entry, GptError, Header};
use ultar_slots::layout::{self, BOOTCTL_PART};
use ultar_slots::Slot;

/// Largest block size the loader works with (records on `ULTAR-BOOTCTL` are 32 KiB apart).
pub const MAX_BLOCK: usize = 32 * 1024;

/// Failure of one block operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskError;

/// Whole-disk block access (UEFI Block I/O on the disk that holds the loader).
pub trait Disk {
    /// Bytes per block (512 or 4096 in practice).
    fn block_size(&self) -> usize;
    /// Index of the last block.
    fn last_block(&self) -> u64;
    /// Reads `buf.len()` bytes (a multiple of the block size) from block `lba` on.
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), DiskError>;
    /// Writes `buf` (a multiple of the block size) from block `lba` on.
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), DiskError>;
    /// Makes every completed write durable (device cache flushed).
    fn flush(&mut self) -> Result<(), DiskError>;
}

/// Why the loader cannot find its partitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutError {
    /// Block size outside 512 … [`MAX_BLOCK`] or not a power of two.
    BlockSize,
    /// Reading the disk failed.
    Io,
    /// Neither the primary nor the backup partition table is valid.
    Gpt(GptError),
    /// `ULTAR-BOOTCTL` is missing or too small.
    NoBootControl,
}

/// The partitions of the A/B layout (positions in blocks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parts {
    /// `ULTAR-BOOTCTL`.
    pub bootctl: Entry,
    /// `ultar-kernel-a`, `ultar-kernel-b` (absent when the disk lacks them).
    pub kernels: [Option<Entry>; 2],
    /// GPT disk GUID; handed to the kernel so it uses this disk's partitions only.
    pub disk_guid: [u8; 16],
}

impl Parts {
    /// The kernel partition of `slot`.
    pub fn kernel(&self, slot: Slot) -> Option<Entry> {
        self.kernels[slot.index()]
    }
}

fn read_table<D: Disk>(
    disk: &mut D,
    lba: u64,
    scratch: &mut [u8],
) -> Result<(Header, usize), LayoutError> {
    let bs = disk.block_size();
    disk.read(lba, &mut scratch[..bs])
        .map_err(|_| LayoutError::Io)?;
    let header = Header::parse(&scratch[..bs], lba).map_err(LayoutError::Gpt)?;
    let len = header.entries_len();
    let blocks = len.div_ceil(bs);
    let area = scratch
        .get_mut(..blocks * bs)
        .ok_or(LayoutError::Gpt(GptError::Geometry))?;
    disk.read(header.entries_lba, area)
        .map_err(|_| LayoutError::Io)?;
    header
        .check_entries(&scratch[..len])
        .map_err(LayoutError::Gpt)?;
    Ok((header, len))
}

/// Finds the partitions. `scratch` must hold the entry array (64 KiB suffice for any table
/// [`Header::parse`] accepts). The backup table is used when the primary one is damaged.
pub fn find_parts<D: Disk>(disk: &mut D, scratch: &mut [u8]) -> Result<Parts, LayoutError> {
    let bs = disk.block_size();
    if !(512..=MAX_BLOCK).contains(&bs) || !bs.is_power_of_two() {
        return Err(LayoutError::BlockSize);
    }
    let last = disk.last_block();
    let (header, len) = match read_table(disk, 1, scratch) {
        Ok(found) => found,
        Err(LayoutError::Io) => return Err(LayoutError::Io),
        Err(_) => read_table(disk, last, scratch)?,
    };
    let entries = &scratch[..len];
    let find = |name: &str| header.find(entries, name).map_err(LayoutError::Gpt);
    let bootctl = find(BOOTCTL_PART)?.ok_or(LayoutError::NoBootControl)?;
    if bootctl.blocks().saturating_mul(bs as u64) < layout::BOOTCTL_MIN_BYTES {
        return Err(LayoutError::NoBootControl);
    }
    Ok(Parts {
        bootctl,
        kernels: [
            find(layout::kernel_part(Slot::A))?,
            find(layout::kernel_part(Slot::B))?,
        ],
        disk_guid: header.disk_guid,
    })
}
