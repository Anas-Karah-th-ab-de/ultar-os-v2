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
    let mut h = Header {
        my_lba: 1,
        alternate_lba: n - 1,
        first_usable: 34,
        last_usable: n - 34,
        entries_lba: 2,
        entry_count: ENTRY_COUNT as u32,
        entry_size: ENTRY_SIZE as u32,
        entries_crc: crate::crc32(&entries),
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
    front[512..1024].copy_from_slice(&build_header(&h, &entries));
    front[1024..].copy_from_slice(&entries);
    h.my_lba = n - 1;
    h.alternate_lba = 1;
    h.entries_lba = n - 33;
    back.fill(0);
    back[..entries.len()].copy_from_slice(&entries);
    back[32 * 512..].copy_from_slice(&build_header(&h, &entries));
    Some(n - 33)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpt::Header;

    fn source() -> Source {
        Source {
            esp: (2048, 4095),
            kernel: (4096, 8191),
            system: (8192, 40959),
            data: (40960, 49151),
        }
    }

    #[test]
    fn plan_has_the_ab_layout_on_mib_boundaries() {
        let p = plan(&source());
        let names: std::vec::Vec<&str> = parts(&p).map(|t| t.name).collect();
        assert_eq!(
            names,
            [
                "ULTAR-EFI",
                "ULTAR-BOOTCTL",
                "ultar-kernel-a",
                "ultar-kernel-b",
                "ultar-system-a",
                "ultar-system-b",
                "ultar-data"
            ]
        );
        for t in parts(&p) {
            assert_eq!(t.first % MIB, 0, "{}", t.name);
        }
        assert_eq!(p[2].sectors, 32 * MIB, "kernel slot at least 32 MiB");
        assert_eq!(p[4].sectors, 64 * MIB, "system slot at least 64 MiB");
        assert_eq!(p[2].fill, Fill::Copy((4096, 8191)));
        assert_eq!(p[3].fill, Fill::Zero);
        assert_eq!(needed(&p), p[6].first + p[6].sectors + 34);
    }

    #[test]
    fn source_needs_all_four_partitions() {
        let s = Source::find(|n| (n != "ultar-kernel").then_some((1, 2)));
        assert_eq!(s, Err(SourceError::Missing("ultar-kernel")));
        assert_eq!(
            Source::find(|_| Some((5, 2))),
            Err(SourceError::Inverted("ULTAR-EFI"))
        );
        assert!(Source::find(|_| Some((1, 2))).is_ok());
    }

    #[test]
    fn gpt_round_trips_and_refuses_small_disks() {
        let p = plan(&source());
        let n = needed(&p) + 100;
        let (mut front, mut back) = ([0u8; FRONT_BYTES], [0u8; BACK_BYTES]);
        let guids = [[7u8; 16]; PARTS];
        assert_eq!(
            gpt(&p, needed(&p) - 1, &guids, [1; 16], &mut front, &mut back),
            None
        );
        let back_lba = gpt(&p, n, &guids, [1; 16], &mut front, &mut back).unwrap();
        assert_eq!(back_lba, n - 33);
        assert_eq!(&front[510..512], &[0x55, 0xAA]);
        let h = Header::parse(&front[512..1024], 1).unwrap();
        h.check_entries(&front[1024..1024 + h.entries_len()])
            .unwrap();
        let e = h.find(&front[1024..], "ultar-data").unwrap().unwrap();
        assert_eq!(e.first_lba, p[6].first);
        let hb = Header::parse(&back[32 * 512..], n - 1).unwrap();
        assert_eq!(hb.entries_lba, n - 33);
    }

    #[test]
    fn bootctl_image_holds_the_factory_table_twice() {
        let mut small = [0u8; 16];
        assert!(!bootctl_image(&mut small));
        let mut img = std::vec![0xffu8; 1024 * 1024];
        assert!(bootctl_image(&mut img));
        let t = SlotTable::factory().encode();
        assert_eq!(&img[..t.len()], &t[..]);
        assert_eq!(&img[32 * 1024..32 * 1024 + t.len()], &t[..]);
        assert!(img[64 * 1024..].iter().all(|b| *b == 0));
    }
}
