//! The installer's target layout, one pure source for the kernel's text installer
//! (`kernel/ultar-kern/src/install_ab.rs`) and the graphical installer
//! (`platform/ultar-installer-ui`): which partitions the target disk gets, how large, what
//! fills them, the factory `ULTAR-BOOTCTL` image and the GPT (protective MBR, primary and
//! backup). No I/O, no allocation: callers pass buffers and GUIDs.

use crate::gpt::{build_entry, build_header, Header};
use crate::layout::{
    self, BOOTCTL_PART, DATA_PART, EFI_PART, LEGACY_ROOT_PART, META_OFFSETS, TABLE_COPY_OFFSETS,
};
use crate::{Slot, SlotTable};

/// Sectors per MiB (512-byte sectors); partitions start on MiB boundaries.
pub const MIB: u64 = 2048;
/// Sector size the layout is computed in.
pub const SECTOR: u64 = 512;
/// Number of target partitions.
pub const PARTS: usize = 8;
/// Entries of the written partition table (128 × 128 bytes = 16 KiB).
pub const ENTRY_COUNT: usize = 128;
/// Size of one entry.
pub const ENTRY_SIZE: usize = 128;
/// Bytes of the front area: protective MBR, header, entries (LBA 0..33).
pub const FRONT_BYTES: usize = 34 * 512;
/// Bytes of the back area: entries and header (last 33 LBAs).
pub const BACK_BYTES: usize = 33 * 512;
/// Medium partition with the kernel image for slot A.
pub const SRC_KERNEL: &str = "ultar-kernel";

/// EFI system partition.
pub const TYPE_ESP: [u8; 16] = [
    0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b,
];
/// Linux file system data (ext4 system and data partitions).
pub const TYPE_LINUX: [u8; 16] = [
    0xaf, 0x3d, 0xc6, 0x0f, 0x83, 0x84, 0x72, 0x47, 0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47, 0x7d, 0xe4,
];
/// Ultar raw boot data (slot table, kernel images).
pub const TYPE_ULTAR_RAW: [u8; 16] = [
    0x55, 0x4c, 0x54, 0x52, 0x2d, 0x42, 0x4f, 0x4f, 0x91, 0x7a, 0x3e, 0x52, 0x0c, 0x6d, 0xab, 0x01,
];

/// A partition of the medium: first and last sector.
pub type Range = (u64, u64);

/// The medium's four partitions the installation is made from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    /// `ULTAR-EFI`.
    pub esp: Range,
    /// `ultar-kernel`.
    pub kernel: Range,
    /// `ultar-compat` (system).
    pub system: Range,
    /// `ultar-data`.
    pub data: Range,
}

/// What the medium lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceError {
    /// Partition missing.
    Missing(&'static str),
    /// First sector after last.
    Inverted(&'static str),
}

impl Source {
    /// The medium from a lookup of partitions by name.
    pub fn find(mut part: impl FnMut(&str) -> Option<Range>) -> Result<Source, SourceError> {
        let mut get = |name: &'static str| -> Result<Range, SourceError> {
            let r = part(name).ok_or(SourceError::Missing(name))?;
            if r.0 > r.1 {
                return Err(SourceError::Inverted(name));
            }
            Ok(r)
        };
        Ok(Source {
            esp: get(EFI_PART)?,
            kernel: get(SRC_KERNEL)?,
            system: get(LEGACY_ROOT_PART)?,
            data: get(DATA_PART)?,
        })
    }
}

/// What fills a target partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    /// Copy (and verify) this medium range.
    Copy(Range),
    /// Zero the first MiB (an empty slot must not look like a file system or image).
    Zero,
    /// The factory boot control data ([`bootctl_image`]).
    BootCtl,
}

/// One partition of the target disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    /// GPT name.
    pub name: &'static str,
    /// First sector.
    pub first: u64,
    /// Length in sectors.
    pub sectors: u64,
    /// Type GUID.
    pub type_guid: [u8; 16],
    /// Content.
    pub fill: Fill,
}

fn len(r: Range) -> u64 {
    r.1 - r.0 + 1
}

/// Rounds up to whole MiB.
pub fn align(sectors: u64) -> u64 {
    sectors.div_ceil(MIB) * MIB
}

/// The target layout: slot sizes leave room for later releases (kernel slot at least
/// 32 MiB, system slot twice the medium's system and at least 64 MiB).
pub fn plan(src: &Source) -> [Target; PARTS] {
    let kernel = align(len(src.kernel).max(layout::KERNEL_PART_INSTALL_BYTES / SECTOR));
    let system = align((2 * len(src.system)).max(64 * MIB));
    let rows: [(&'static str, u64, [u8; 16], Fill); PARTS] = [
        (EFI_PART, align(len(src.esp)), TYPE_ESP, Fill::Copy(src.esp)),
        (
            BOOTCTL_PART,
            layout::BOOTCTL_INSTALL_BYTES / SECTOR,
            TYPE_ULTAR_RAW,
            Fill::BootCtl,
        ),
        (
            layout::kernel_part(Slot::A),
            kernel,
            TYPE_ULTAR_RAW,
            Fill::Copy(src.kernel),
        ),
        (
            layout::kernel_part(Slot::B),
            kernel,
            TYPE_ULTAR_RAW,
            Fill::Zero,
        ),
        (
            layout::system_part(Slot::A),
            system,
            TYPE_LINUX,
            Fill::Copy(src.system),
        ),
        (layout::system_part(Slot::B), system, TYPE_LINUX, Fill::Zero),
        (
            DATA_PART,
            align(len(src.data)),
            TYPE_LINUX,
            Fill::Copy(src.data),
        ),
        ("", 0, [0; 16], Fill::Zero),
    ];
    let mut pos = MIB;
    rows.map(|(name, sectors, type_guid, fill)| {
        let t = Target {
            name,
            first: pos,
            sectors,
            type_guid,
            fill,
        };
        pos += sectors;
        t
    })
}

/// Where the partitions of a plan go when the disk is not erased as a whole (installing
/// beside another system, or manual partitioning): one region per role, each a first sector
/// and a length in sectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Regions {
    /// The EFI system partition `ULTAR-EFI`.
    pub esp: (u64, u64),
    /// Slot A: `ULTAR-BOOTCTL`, `ultar-kernel-a` and `ultar-system-a`, in this order.
    pub slot_a: (u64, u64),
    /// Slot B: `ultar-kernel-b` and `ultar-system-b`.
    pub slot_b: (u64, u64),
    /// `ultar-data` (the whole region; the medium's data is copied to its start).
    pub data: (u64, u64),
}

/// Why regions cannot hold the A/B layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionError {
    /// A region does not start on a MiB boundary (role).
    Unaligned(&'static str),
    /// A region is smaller than its partitions need (role, needed sectors, given sectors).
    TooSmall(&'static str, u64, u64),
    /// Two regions overlap (roles).
    Overlap(&'static str, &'static str),
}

/// Sectors each role needs at least for `src`: ESP, slot A, slot B, data.
pub fn minimum(src: &Source) -> [u64; 4] {
    let p = plan(src);
    let boot = layout::BOOTCTL_INSTALL_BYTES / SECTOR;
    [
        p[0].sectors,
        boot + p[2].sectors + p[4].sectors,
        p[3].sectors + p[5].sectors,
        p[6].sectors,
    ]
}

/// The A/B layout placed into `r`: the kernel slots keep the size of [`plan`], each system
/// slot takes the rest of its slot region, the data partition its whole region.
pub fn plan_in(src: &Source, r: &Regions) -> Result<[Target; PARTS], RegionError> {
    let base = plan(src);
    let need = minimum(src);
    let roles: [(&'static str, (u64, u64), u64); 4] = [
        (EFI_PART, r.esp, need[0]),
        ("slot-a", r.slot_a, need[1]),
        ("slot-b", r.slot_b, need[2]),
        (DATA_PART, r.data, need[3]),
    ];
    for (name, (first, sectors), min) in roles {
        if first % MIB != 0 || first == 0 {
            return Err(RegionError::Unaligned(name));
        }
        if sectors < min {
            return Err(RegionError::TooSmall(name, min, sectors));
        }
    }
    for i in 0..roles.len() {
        for j in i + 1..roles.len() {
            let (a, b) = (roles[i].1, roles[j].1);
            if a.0 < b.0 + b.1 && b.0 < a.0 + a.1 {
                return Err(RegionError::Overlap(roles[i].0, roles[j].0));
            }
        }
    }
    let boot = layout::BOOTCTL_INSTALL_BYTES / SECTOR;
    let kernel = base[2].sectors;
    let mut out = base;
    out[0].first = r.esp.0;
    out[1].first = r.slot_a.0;
    out[2].first = r.slot_a.0 + boot;
    out[4].first = out[2].first + kernel;
    out[4].sectors = r.slot_a.1 - boot - kernel;
    out[3].first = r.slot_b.0;
    out[5].first = r.slot_b.0 + kernel;
    out[5].sectors = r.slot_b.1 - kernel;
    out[6].first = r.data.0;
    out[6].sectors = r.data.1;
    Ok(out)
}

/// The A/B layout in one free region of `free` sectors starting at `first` (beside another
/// system): ESP, slot A, slot B as [`plan`] sizes them, the data partition takes `share`
/// percent of the region less those (at least the medium's data). `None` when it does not fit.
pub fn plan_beside(src: &Source, first: u64, free: u64, share: u8) -> Option<[Target; PARTS]> {
    let start = first.div_ceil(MIB) * MIB;
    let usable = (first + free).checked_sub(start)?;
    let need = minimum(src);
    let fixed = need[0] + need[1] + need[2];
    let budget = usable.checked_mul(u64::from(share.min(100)))? / 100;
    let data = (budget.saturating_sub(fixed) / MIB * MIB).max(need[3]);
    if fixed + data > usable {
        return None;
    }
    let r = Regions {
        esp: (start, need[0]),
        slot_a: (start + need[0], need[1]),
        slot_b: (start + need[0] + need[1], need[2]),
        data: (start + fixed, data),
    };
    plan_in(src, &r).ok()
}

/// Writes the GPT of a disk that keeps some partitions: `kept[i]` is the 128-byte entry of
/// index `i` to keep (all zero = free), the plan's partitions go into the free indices in
/// order. Same output as [`gpt`]; `None` when the disk is too small, an entry index is out of
/// range, there are not enough free indices or a planned partition overlaps a kept one or
/// lies outside the usable sectors.
#[allow(clippy::too_many_arguments)]
pub fn gpt_keeping(
    plan: &[Target; PARTS],
    kept: &[[u8; ENTRY_SIZE]; ENTRY_COUNT],
    n: u64,
    guids: &[[u8; 16]; PARTS],
    disk: [u8; 16],
    front: &mut [u8; FRONT_BYTES],
    back: &mut [u8; BACK_BYTES],
) -> Option<u64> {
    let last_usable = n.checked_sub(34)?;
    let mut entries = [0u8; ENTRY_COUNT * ENTRY_SIZE];
    let mut ranges = [(0u64, 0u64); ENTRY_COUNT];
    let mut used = 0;
    for (i, e) in kept.iter().enumerate() {
        if e.iter().all(|b| *b == 0) {
            continue;
        }
        let first = u64::from_le_bytes(e[32..40].try_into().ok()?);
        let last = u64::from_le_bytes(e[40..48].try_into().ok()?);
        if first < 34 || last < first || last > last_usable {
            return None;
        }
        entries[i * ENTRY_SIZE..(i + 1) * ENTRY_SIZE].copy_from_slice(e);
        ranges[used] = (first, last);
        used += 1;
    }
    let mut slot = 0;
    for (k, t) in parts(plan).enumerate() {
        let (first, last) = (t.first, t.first + t.sectors - 1);
        if first < 34 || last > last_usable {
            return None;
        }
        if ranges[..used].iter().any(|r| first <= r.1 && r.0 <= last) {
            return None;
        }
        while slot < ENTRY_COUNT && kept[slot].iter().any(|b| *b != 0) {
            slot += 1;
        }
        if slot == ENTRY_COUNT {
            return None;
        }
        build_entry(
            &mut entries[slot * ENTRY_SIZE..],
            t.type_guid,
            guids[k],
            first,
            last,
            t.name,
        );
        slot += 1;
    }
    tables(&entries, n, disk, front, back);
    Some(n - 33)
}

fn tables(
    entries: &[u8; ENTRY_COUNT * ENTRY_SIZE],
    n: u64,
    disk: [u8; 16],
    front: &mut [u8; FRONT_BYTES],
    back: &mut [u8; BACK_BYTES],
) {
    let mut h = Header {
        my_lba: 1,
        alternate_lba: n - 1,
        first_usable: 34,
        last_usable: n - 34,
        entries_lba: 2,
        entry_count: ENTRY_COUNT as u32,
        entry_size: ENTRY_SIZE as u32,
        entries_crc: crate::crc32(entries),
        disk_guid: disk,
    };
    front.fill(0);
    let m = &mut front[446..512];
    m[1..4].copy_from_slice(&[0x00, 0x02, 0x00]);
    m[4] = 0xEE;
    m[5..8].copy_from_slice(&[0xff, 0xff, 0xff]);
    m[8..12].copy_from_slice(&1u32.to_le_bytes());
    m[12..16].copy_from_slice(&((n - 1).min(0xffff_ffff) as u32).to_le_bytes());
    m[64] = 0x55;
    m[65] = 0xAA;
    front[512..1024].copy_from_slice(&build_header(&h, entries));
    front[1024..].copy_from_slice(entries);
    h.my_lba = n - 1;
    h.alternate_lba = 1;
    h.entries_lba = n - 33;
    back.fill(0);
    back[..entries.len()].copy_from_slice(entries);
    back[32 * 512..].copy_from_slice(&build_header(&h, entries));
}

/// The partitions of a plan that exist (non-empty).
pub fn parts(plan: &[Target; PARTS]) -> impl Iterator<Item = &Target> {
    plan.iter().filter(|t| t.sectors > 0)
}

/// Sectors the target disk needs (layout + backup partition table).
pub fn needed(plan: &[Target; PARTS]) -> u64 {
    parts(plan).map(|t| t.first + t.sectors).max().unwrap_or(0) + 34
}

/// Fills `img` (≥ [`layout::BOOTCTL_MIN_BYTES`]) with the factory boot control data: the
/// factory table in both copies, empty slot records, zeros elsewhere. `false` when too small.
pub fn bootctl_image(img: &mut [u8]) -> bool {
    if (img.len() as u64) < layout::BOOTCTL_MIN_BYTES {
        return false;
    }
    img.fill(0);
    let table = SlotTable::factory().encode();
    for off in TABLE_COPY_OFFSETS {
        img[off as usize..off as usize + table.len()].copy_from_slice(&table);
    }
    for off in META_OFFSETS {
        img[off as usize..off as usize + 64].fill(0);
    }
    true
}

/// Writes protective MBR + primary GPT into `front` and backup GPT into `back` for a disk of
/// `n` sectors; `guids[i]` is the unique GUID of partition `i`, `disk` the disk GUID.
/// Returns the first sector of `back`, or `None` when the disk is too small for the plan.
pub fn gpt(
    plan: &[Target; PARTS],
    n: u64,
    guids: &[[u8; 16]; PARTS],
    disk: [u8; 16],
    front: &mut [u8; FRONT_BYTES],
    back: &mut [u8; BACK_BYTES],
) -> Option<u64> {
    if n < needed(plan) {
        return None;
    }
    let mut entries = [0u8; ENTRY_COUNT * ENTRY_SIZE];
    for (i, t) in parts(plan).enumerate() {
        let out = &mut entries[i * ENTRY_SIZE..];
        build_entry(
            out,
            t.type_guid,
            guids[i],
            t.first,
            t.first + t.sectors - 1,
            t.name,
        );
    }
    tables(&entries, n, disk, front, back);
    Some(n - 33)
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
