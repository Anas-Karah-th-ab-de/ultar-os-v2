//! Boot control on raw partitions: the backend for a device started by the Ultar loader.
//!
//! The disk carries the A/B layout of `ultar_slots::layout` (`ULTAR-BOOTCTL`,
//! `ultar-kernel-a/b`, `ultar-system-a/b`). The running system offers the partitions as nodes
//! named by their GPT name in one directory (on the Ultar kernel `partition_dir` in the
//! configuration, root only; on Linux `/dev/disk/by-partlabel` works the same way). The kernel
//! names the slot it was started from in a small text file (`running_slot_file`, `a` or `b`).
//!
//! Durability: every change is written in place and followed by `sync_all`, which on the Ultar
//! kernel is a device flush (NVMe FLUSH, ATA FLUSH CACHE); the slot table is written with the
//! two-copy rule of [`crate::slottable::plan_store`], the same code the loader runs.
//! The running slot's partitions are refused here and, independently, by the kernel.

use crate::bootctl::{BootControl, Slot, SlotMeta, SlotState, SlotWrite};
use crate::error::UpdateError;
use crate::fsutil::read_bounded;
use crate::manifest::Role;
use crate::slottable::{plan_store, select, SlotFlags, SlotTable, TABLE_LEN};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use ultar_slots::layout::{self, BOOTCTL_PART, META_OFFSETS, TABLE_COPY_OFFSETS};
use ultar_slots::meta::{SlotRecord, META_LEN};

/// Boot control over the partitions of the A/B layout. See the module documentation.
#[derive(Debug)]
pub struct PartitionBootControl {
    dir: PathBuf,
    running: Slot,
    roles: Vec<Role>,
}

struct PartitionWriter {
    file: File,
    offset: u64,
    remaining: u64,
}

impl Write for PartitionWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.len() as u64 > self.remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "write beyond the announced payload size",
            ));
        }
        let n = self.file.write_at(buf, self.offset)?;
        self.offset += n as u64;
        self.remaining -= n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl SlotWrite for PartitionWriter {
    fn finish(&mut self) -> Result<(), UpdateError> {
        self.file.sync_all()?;
        Ok(())
    }
}

/// Reads the slot the kernel reports in `file` (`a` or `b`, surrounding white space ignored).
/// A system started without the A/B loader reports `-` or has no such file:
/// [`UpdateError::LayoutNotAb`].
pub fn read_running_slot(file: &Path) -> Result<Slot, UpdateError> {
    match read_bounded(file, 16) {
        Ok(Some(bytes)) => {
            let text = String::from_utf8_lossy(&bytes);
            match text.trim() {
                "-" => Err(UpdateError::LayoutNotAb),
                letter => Slot::from_letter(letter).ok_or(UpdateError::BootControl),
            }
        }
        Ok(None) => Err(UpdateError::LayoutNotAb),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(UpdateError::LayoutNotAb),
        Err(e) => Err(e.into()),
    }
}

fn part_len(file: &mut File) -> Result<u64, UpdateError> {
    Ok(file.seek(SeekFrom::End(0))?)
}

impl PartitionBootControl {
    /// Opens the boot control: `dir` holds the partition nodes, `running` is the slot the
    /// kernel was started from. Fails with [`UpdateError::LayoutNotAb`] when the disk has no
    /// `ULTAR-BOOTCTL` partition, [`UpdateError::BootControl`] when it is too small or holds no
    /// valid table, and [`UpdateError::InvalidArgument`] for a role this layout cannot hold
    /// (only `kernel` and `system`; the start partition is not updated through slots).
    pub fn open(dir: &Path, running: Slot, roles: &[Role]) -> Result<Self, UpdateError> {
        let mut roles = roles.to_vec();
        roles.sort_unstable();
        roles.dedup();
        if roles.is_empty() || roles.contains(&Role::Boot) {
            return Err(UpdateError::InvalidArgument);
        }
        let control = Self {
            dir: dir.to_path_buf(),
            running,
            roles,
        };
        let mut file = match File::open(control.node(BOOTCTL_PART)) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(UpdateError::LayoutNotAb),
            Err(e) => return Err(e.into()),
        };
        if part_len(&mut file)? < layout::BOOTCTL_MIN_BYTES {
            return Err(UpdateError::BootControl);
        }
        control.table()?;
        Ok(control)
    }

    /// Like [`PartitionBootControl::open`] with the running slot read from `slot_file`
    /// ([`read_running_slot`]).
    pub fn open_running(dir: &Path, slot_file: &Path, roles: &[Role]) -> Result<Self, UpdateError> {
        Self::open(dir, read_running_slot(slot_file)?, roles)
    }

    fn node(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn part_name(role: Role, slot: Slot) -> Result<&'static str, UpdateError> {
        match role {
            Role::Kernel => Ok(layout::kernel_part(slot)),
            Role::System => Ok(layout::system_part(slot)),
            Role::Boot => Err(UpdateError::InvalidArgument),
        }
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>, UpdateError> {
        let file = File::open(self.node(BOOTCTL_PART))?;
        let mut buf = vec![0u8; len];
        file.read_exact_at(&mut buf, offset)?;
        Ok(buf)
    }

    fn write_at(&self, offset: u64, bytes: &[u8]) -> Result<(), UpdateError> {
        let file = OpenOptions::new()
            .write(true)
            .open(self.node(BOOTCTL_PART))?;
        file.write_all_at(bytes, offset)?;
        file.sync_all()?;
        Ok(())
    }

    fn copies(&self) -> Result<[Option<SlotTable>; 2], UpdateError> {
        let mut out = [None, None];
        for (slot, offset) in out.iter_mut().zip(TABLE_COPY_OFFSETS) {
            *slot = SlotTable::decode(&self.read_at(offset, TABLE_LEN)?).ok();
        }
        Ok(out)
    }

    /// The current slot table. [`UpdateError::BootControl`] when both copies are invalid.
    pub fn table(&self) -> Result<SlotTable, UpdateError> {
        SlotTable::newest(self.copies()?).ok_or(UpdateError::BootControl)
    }

    fn store(&self, table: SlotTable) -> Result<(), UpdateError> {
        match plan_store(self.copies()?, table).map_err(|_| UpdateError::BootControl)? {
            Some((copy, next)) => self.write_at(TABLE_COPY_OFFSETS[copy], &next.encode()),
            None => Ok(()),
        }
    }

    fn change(&self, edit: impl FnOnce(&mut SlotTable)) -> Result<(), UpdateError> {
        let mut table = self.table()?;
        edit(&mut table);
        self.store(table)
    }

    /// The verity root hash recorded for `slot` when it was made bootable. `Ok(None)` for a
    /// slot without a record (the factory slot) or a release without verity; a damaged record
    /// of a bootable slot is [`UpdateError::BootControl`].
    pub fn verity_root(&self, slot: Slot) -> Result<Option<[u8; 32]>, UpdateError> {
        let bytes = self.read_at(META_OFFSETS[slot.index()], META_LEN)?;
        if bytes.iter().all(|b| *b == 0) {
            return Ok(None);
        }
        match SlotRecord::decode(&bytes, slot) {
            Ok(record) => Ok(record.verity_root),
            Err(_) if !self.table()?.slot(slot).bootable => Ok(None),
            Err(_) => Err(UpdateError::BootControl),
        }
    }

    /// Writes the factory state on a freshly laid out `ULTAR-BOOTCTL` node: both table copies
    /// ([`SlotTable::factory`]) and empty slot records. For installers and tests; refuses a
    /// partition that already holds a valid table, so a running device is never reset.
    pub fn format(dir: &Path) -> Result<(), UpdateError> {
        let path = dir.join(BOOTCTL_PART);
        let mut file = OpenOptions::new().read(true).write(true).open(&path)?;
        if part_len(&mut file)? < layout::BOOTCTL_MIN_BYTES {
            return Err(UpdateError::BootControl);
        }
        let mut copy = [0u8; TABLE_LEN];
        for offset in TABLE_COPY_OFFSETS {
            file.read_exact_at(&mut copy, offset)?;
            if SlotTable::decode(&copy).is_ok() {
                return Err(UpdateError::InvalidArgument);
            }
        }
        for offset in META_OFFSETS {
            file.write_all_at(&[0u8; META_LEN], offset)?;
        }
        let table = SlotTable::factory().encode();
        for offset in TABLE_COPY_OFFSETS {
            file.write_all_at(&table, offset)?;
        }
        file.sync_all()?;
        Ok(())
    }

    /// What the loader does at a start, for tests and VM set-ups that have no loader: takes
    /// the start decision, stores it with the two-copy rule and returns the slot to start
    /// ([`UpdateError::NoBootableSlot`] when nothing can start). The UEFI loader runs the same
    /// two functions (`select`, `plan_store`) from `ultar-slots`.
    pub fn loader_start(dir: &Path) -> Result<Slot, UpdateError> {
        let probe = Self {
            dir: dir.to_path_buf(),
            running: Slot::A,
            roles: vec![Role::System],
        };
        let (slot, next) = select(&probe.table()?).ok_or(UpdateError::NoBootableSlot)?;
        probe.store(next)?;
        Ok(slot)
    }
}

impl BootControl for PartitionBootControl {
    fn active_slot(&self) -> Result<Slot, UpdateError> {
        Ok(self.running)
    }

    fn slot_state(&self, slot: Slot) -> Result<SlotState, UpdateError> {
        let table = self.table()?;
        let flags = table.slot(slot);
        Ok(SlotState {
            bootable: flags.bootable,
            successful: flags.successful,
            trial_tries: table.trial.filter(|(s, _)| *s == slot).map(|(_, t)| t),
            is_default: table.default == slot,
        })
    }

    fn roles(&self) -> Vec<Role> {
        self.roles.clone()
    }

    fn mark_unbootable(&mut self, slot: Slot) -> Result<(), UpdateError> {
        // The running slot may only be given up while it is a trial: the default slot is
        // then the other one, and the next start goes there.
        if slot == self.running && self.table()?.default == slot {
            return Err(UpdateError::InvalidArgument);
        }
        self.change(|table| {
            *table.slot_mut(slot) = SlotFlags::default();
            if table.trial.is_some_and(|(s, _)| s == slot) {
                table.trial = None;
            }
        })
    }

    fn open_writer(
        &mut self,
        slot: Slot,
        role: Role,
        size: u64,
    ) -> Result<Box<dyn SlotWrite>, UpdateError> {
        if slot == self.running || self.table()?.slot(slot).bootable || !self.roles.contains(&role)
        {
            return Err(UpdateError::InvalidArgument);
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.node(Self::part_name(role, slot)?))?;
        let free = part_len(&mut file)?;
        if size > free {
            return Err(UpdateError::NoSpace { needed: size, free });
        }
        Ok(Box::new(PartitionWriter {
            file,
            offset: 0,
            remaining: size,
        }))
    }

    fn open_reader(&self, slot: Slot, role: Role) -> Result<Box<dyn Read>, UpdateError> {
        if !self.roles.contains(&role) {
            return Err(UpdateError::InvalidArgument);
        }
        Ok(Box::new(File::open(
            self.node(Self::part_name(role, slot)?),
        )?))
    }

    fn mark_bootable(&mut self, slot: Slot, meta: &SlotMeta) -> Result<(), UpdateError> {
        if slot == self.running {
            return Err(UpdateError::InvalidArgument);
        }
        let record = SlotRecord {
            slot,
            verity_root: meta.verity_root,
        };
        self.write_at(META_OFFSETS[slot.index()], &record.encode())?;
        self.change(|table| {
            *table.slot_mut(slot) = SlotFlags {
                bootable: true,
                successful: false,
            };
        })
    }

    fn set_next_boot_once(&mut self, slot: Slot, tries: u8) -> Result<(), UpdateError> {
        if tries == 0 || slot == self.running || !self.table()?.slot(slot).bootable {
            return Err(UpdateError::InvalidArgument);
        }
        self.change(|table| table.trial = Some((slot, tries)))
    }

    fn mark_successful(&mut self) -> Result<(), UpdateError> {
        let active = self.running;
        if !self.table()?.slot(active).bootable {
            return Err(UpdateError::BootControl);
        }
        self.change(|table| {
            table.slot_mut(active).successful = true;
            table.default = active;
            table.trial = None;
        })
    }

    fn set_default(&mut self, slot: Slot) -> Result<(), UpdateError> {
        let flags = self.table()?.slot(slot);
        if !(flags.bootable && flags.successful) {
            return Err(UpdateError::PreviousUnavailable);
        }
        self.change(|table| {
            table.default = slot;
            table.trial = None;
        })
    }
}

#[cfg(test)]
#[path = "partctl_tests.rs"]
mod tests;
