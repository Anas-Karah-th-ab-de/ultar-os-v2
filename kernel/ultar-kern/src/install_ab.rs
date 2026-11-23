//! Installation in the A/B layout (`ultar_slots::layout`): the medium carries the loader on
//! `ULTAR-EFI`, the kernel image on `ultar-kernel`, the system on `ultar-compat` and the data
//! partition `ultar-data`. On the target this becomes:
//!
//! ```text
//! ULTAR-EFI       copy of the medium's (loader only)
//! ULTAR-BOOTCTL   1 MiB: factory slot table in both copies, empty slot records
//! ultar-kernel-a  copy of ultar-kernel          ultar-kernel-b  same size, start zeroed
//! ultar-system-a  copy of ultar-compat          ultar-system-b  same size, start zeroed
//! ultar-data      copy of the medium's
//! ```
//!
//! Slot sizes leave room for later releases: a kernel slot is at least 32 MiB, a system slot
//! twice the medium's system (at least 64 MiB). An installation from before A/B on the target
//! disk is replaced as a whole after the confirmation screen (which says the disk is erased);
//! there is no in-place migration (see docs/integration/boot-ab.md).

use super::{t, Platte, Ui, BLOCK, BLOCK_SEKT};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use ultar_slots::gpt::{build_entry, build_header, Header};
use ultar_slots::layout::{self, BOOTCTL_PART, DATA_PART, EFI_PART, META_OFFSETS, TABLE_COPY_OFFSETS};
use ultar_slots::{Slot, SlotTable};

const MIB: u64 = 2048;
/// EFI system partition.
const TYPE_ESP: [u8; 16] = [0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b];
/// Linux file system data (ext4 system and data partitions).
const TYPE_LINUX: [u8; 16] = [0xaf, 0x3d, 0xc6, 0x0f, 0x83, 0x84, 0x72, 0x47, 0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47, 0x7d, 0xe4];
/// Ultar raw boot data (slot table, kernel images): no other system should mount them.
const TYPE_ULTAR_RAW: [u8; 16] = [0x55, 0x4c, 0x54, 0x52, 0x2d, 0x42, 0x4f, 0x4f, 0x91, 0x7a, 0x3e, 0x52, 0x0c, 0x6d, 0xab, 0x01];
/// Medium partition with the kernel image for slot A.
pub(super) const SRC_KERNEL: &str = "ultar-kernel";

/// What fills a target partition.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Fill {
    /// Copy (and verify) the medium partition with these first/last sectors.
    Copy(u64, u64),
    /// Zero the first sectors (an empty slot must not look like a file system or image).
    Zero,
    /// The factory boot control data.
    BootCtl,
}

/// One partition of the target disk.
#[derive(Clone, Copy)]
pub(super) struct Target {
    pub(super) name: &'static str,
    pub(super) first: u64,
    pub(super) sectors: u64,
    type_guid: [u8; 16],
    pub(super) fill: Fill,
}

/// Whether the medium carries an A/B installation.
pub(super) fn is_source(q: &Platte) -> bool {
    q.teile.iter().any(|t| t.name == SRC_KERNEL)
}

fn align(sectors: u64) -> u64 {
    sectors.div_ceil(MIB) * MIB
}

/// The target layout for medium `q`; `Err` names what the medium lacks.
pub(super) fn plan(q: &Platte) -> Result<Vec<Target>, &'static str> {
    let part = |name: &str| q.teile.iter().find(|t| t.name == name).map(|t| (t.first, t.last));
    let esp = part(EFI_PART).ok_or("medium without ULTAR-EFI")?;
    let kernel = part(SRC_KERNEL).ok_or("medium without ultar-kernel")?;
    let system = part(layout::LEGACY_ROOT_PART).ok_or("medium without ultar-compat")?;
    let data = part(DATA_PART).ok_or("medium without ultar-data")?;
    let len = |(f, l): (u64, u64)| l - f + 1;
    let kernel_size = align(len(kernel).max(layout::KERNEL_PART_INSTALL_BYTES / 512));
    let system_size = align((2 * len(system)).max(64 * MIB));
    let rows: [(&'static str, u64, [u8; 16], Fill); 8] = [
        (EFI_PART, align(len(esp)), TYPE_ESP, Fill::Copy(esp.0, esp.1)),
        (BOOTCTL_PART, layout::BOOTCTL_INSTALL_BYTES / 512, TYPE_ULTAR_RAW, Fill::BootCtl),
        (layout::kernel_part(Slot::A), kernel_size, TYPE_ULTAR_RAW, Fill::Copy(kernel.0, kernel.1)),
        (layout::kernel_part(Slot::B), kernel_size, TYPE_ULTAR_RAW, Fill::Zero),
        (layout::system_part(Slot::A), system_size, TYPE_LINUX, Fill::Copy(system.0, system.1)),
        (layout::system_part(Slot::B), system_size, TYPE_LINUX, Fill::Zero),
        (DATA_PART, align(len(data)), TYPE_LINUX, Fill::Copy(data.0, data.1)),
        ("", 0, [0; 16], Fill::Zero),
    ];
    let mut pos = MIB;
    let mut out = Vec::new();
    for (name, sectors, type_guid, fill) in rows.into_iter().filter(|r| r.1 > 0) {
        out.push(Target { name, first: pos, sectors, type_guid, fill });
        pos += sectors;
    }
    Ok(out)
}

/// Sectors the target disk needs (layout + backup partition table).
pub(super) fn needed(plan: &[Target]) -> u64 {
    plan.last().map_or(0, |t| t.first + t.sectors) + 34
}

/// The 1 MiB image of `ULTAR-BOOTCTL`: the factory table in both copies, empty records.
pub(super) fn bootctl_image() -> Vec<u8> {
    let mut img = vec![0u8; layout::BOOTCTL_INSTALL_BYTES as usize];
    let table = SlotTable::factory().encode();
    for off in TABLE_COPY_OFFSETS {
        img[off as usize..off as usize + table.len()].copy_from_slice(&table);
    }
    for off in META_OFFSETS {
        img[off as usize..off as usize + 64].fill(0);
    }
    img
}

/// Protective MBR + primary GPT (front, LBA 0..33) and backup GPT (back) for `plan` on a disk
/// of `n` sectors; returns (front, back, first sector of back).
pub(super) fn gpt(plan: &[Target], n: u64) -> (Vec<u8>, Vec<u8>, u64) {
    let mut entries = vec![0u8; 128 * 128];
    for (i, t) in plan.iter().enumerate() {
        build_entry(&mut entries[i * 128..], t.type_guid, super::zufall16(), t.first, t.first + t.sectors - 1, t.name);
    }
    let mut h = Header { my_lba: 1, alternate_lba: n - 1, first_usable: 34, last_usable: n - 34, entries_lba: 2, entry_count: 128, entry_size: 128, entries_crc: ultar_slots::crc32(&entries), disk_guid: super::zufall16() };
    let mut front = vec![0u8; 34 * 512];
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
    let mut back = vec![0u8; 33 * 512];
    back[..entries.len()].copy_from_slice(&entries);
    back[32 * 512..].copy_from_slice(&build_header(&h, &entries));
    (front, back, n - 33)
}

impl Ui {
    /// Copies and verifies one medium range to the target (progress shown on the way).
    fn ab_copy(&mut self, q: &Platte, z: &Platte, part: &Target, range: (u64, u64), done: &mut u64, total: u64) -> Result<(), String> {
        for verify in [false, true] {
            self.phase_setzen(alloc::format!("{} {}", t(self.lang, if verify { "ph_verify" } else { "ph_copy" }), part.name));
            let mut lba = range.0;
            while lba <= range.1 {
                let n = (range.1 - lba + 1).min(BLOCK_SEKT);
                let mut buf = vec![0u8; n as usize * 512];
                let target = part.first + (lba - range.0);
                let mut b = BLOCK.lock();
                b[q.idx].read(lba, &mut buf).map_err(|e| alloc::format!("{} read: {e}", q.name))?;
                if verify {
                    let mut back = vec![0u8; buf.len()];
                    b[z.idx].read(target, &mut back).map_err(|e| alloc::format!("{} read: {e}", z.name))?;
                    if back != buf {
                        return Err(alloc::format!("verification failed: {}, sector {lba}", part.name));
                    }
                } else {
                    b[z.idx].write(target, &buf).map_err(|e| alloc::format!("{} write: {e}", z.name))?;
                }
                drop(b);
                lba += n;
                *done += n;
            }
            self.fortschritt = (*done, total);
        }
        Ok(())
    }

    /// Writes `data` at the start of `t` and reads it back.
    fn ab_write(&mut self, z: &Platte, t: &Target, data: &[u8]) -> Result<(), String> {
        let mut b = BLOCK.lock();
        b[z.idx].write(t.first, data).map_err(|e| alloc::format!("{}: {e}", t.name))?;
        let mut back = vec![0u8; data.len()];
        b[z.idx].read(t.first, &mut back).map_err(|e| alloc::format!("{}: {e}", t.name))?;
        if back != data {
            return Err(alloc::format!("verification failed: {}", t.name));
        }
        Ok(())
    }

    /// The A/B installation: partitions, boot control, partition table, device flush.
    pub(super) fn installieren_ab(&mut self, q: Platte, z: Platte) -> Result<(), String> {
        let plan = plan(&q).map_err(String::from)?;
        if z.sectors < needed(&plan) {
            return Err(alloc::format!("target disk too small ({} MB)", z.mb));
        }
        crate::kprintln!("[installation] A/B: {} -> {} ({} MB)", q.name, z.name, z.mb);
        let total: u64 = plan.iter().map(|t| if let Fill::Copy(f, l) = t.fill { 2 * (l - f + 1) } else { 0 }).sum::<u64>() + 1;
        let mut done = 0u64;
        // an interrupted installation must not look valid: old partition table first
        self.ab_write(&z, &Target { name: "GPT", first: 0, sectors: 34, type_guid: [0; 16], fill: Fill::Zero }, &vec![0u8; 34 * 512])?;
        for part in &plan {
            match part.fill {
                Fill::Copy(f, l) => self.ab_copy(&q, &z, part, (f, l), &mut done, total)?,
                Fill::Zero => self.ab_write(&z, part, &vec![0u8; (part.sectors.min(MIB) * 512) as usize])?,
                Fill::BootCtl => self.ab_write(&z, part, &bootctl_image())?,
            }
        }
        self.phase_setzen(String::from(t(self.lang, "ph_gpt")));
        let (front, back, back_lba) = gpt(&plan, z.sectors);
        {
            let mut b = BLOCK.lock();
            b[z.idx].write(back_lba, &back).map_err(|e| alloc::format!("GPT back: {e}"))?;
            b[z.idx].write(0, &front).map_err(|e| alloc::format!("GPT front: {e}"))?;
            if let Err(e) = b[z.idx].flush() {
                crate::kprintln!("[installation] {}: no cache flush ({e}) - writes may still be in the drive cache", z.name);
            }
        }
        let h = super::rd(z.idx, 1, 1).ok_or("GPT read back failed")?;
        Header::parse(&h, 1).map_err(|_| String::from("GPT invalid after writing"))?;
        self.fortschritt = (total, total);
        crate::kprintln!("[installation] fertig: A/B layout, {} partitions, slot A active", plan.len());
        Ok(())
    }
}

