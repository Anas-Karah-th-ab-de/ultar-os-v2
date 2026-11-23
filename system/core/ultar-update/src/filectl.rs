//! File-based boot control: two slots whose images are files, and the slot table in two
//! redundant copies.
//!
//! This is the complete reference backend. It is what the tests use, and what a VM or
//! development set-up uses, where the "boot code" is whoever starts the system and calls
//! [`FileBootControl::simulate_boot`] first. Directory layout:
//!
//! ```text
//! <root>/slots.0, slots.1        slot table copies (64 bytes each)
//! <root>/slot-a/<role>.img       payload areas of slot A
//! <root>/slot-a/verity           signed verity root hash of slot A ("-" if none)
//! <root>/slot-b/...
//! ```

use crate::bootctl::{BootControl, Slot, SlotMeta, SlotState, SlotWrite};
use crate::error::UpdateError;
use crate::fsutil::{read_bounded, sync_dir, write_atomic};
use crate::manifest::Role;
use crate::slottable::{select, SlotFlags, SlotTable, TABLE_LEN};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Boot control over plain files. See the module documentation.
#[derive(Debug, Clone)]
pub struct FileBootControl {
    root: PathBuf,
    roles: Vec<Role>,
}

struct FileSlotWriter {
    file: File,
    dir: PathBuf,
    remaining: u64,
}

impl Write for FileSlotWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.len() as u64 > self.remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "write beyond the announced payload size",
            ));
        }
        let n = self.file.write(buf)?;
        self.remaining -= n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl SlotWrite for FileSlotWriter {
    fn finish(&mut self) -> Result<(), UpdateError> {
        self.file.sync_all()?;
        sync_dir(&self.dir)?;
        Ok(())
    }
}

impl FileBootControl {
    /// Opens the boot control in `root`; when no slot table exists yet the factory table is
    /// written (slot A active and successful, slot B empty). This is what an installer or a
    /// development set-up calls once.
    pub fn create(root: &Path, roles: &[Role]) -> Result<Self, UpdateError> {
        fs::create_dir_all(root)?;
        let control = Self::unchecked(root, roles);
        let absent = (0..2).all(|i| !control.table_path(i).exists());
        if absent {
            let table = SlotTable::factory().encode();
            for copy in 0..2 {
                control.write_copy(copy, &table)?;
            }
        }
        control.table()?;
        Ok(control)
    }

    /// Opens an existing boot control; fails when no valid slot table copy is found.
    pub fn open(root: &Path, roles: &[Role]) -> Result<Self, UpdateError> {
        let control = Self::unchecked(root, roles);
        control.table()?;
        Ok(control)
    }

    fn unchecked(root: &Path, roles: &[Role]) -> Self {
        let mut roles = roles.to_vec();
        roles.sort_unstable();
        roles.dedup();
        Self {
            root: root.to_path_buf(),
            roles,
        }
    }

    fn table_path(&self, copy: usize) -> PathBuf {
        self.root.join(format!("slots.{copy}"))
    }

    fn slot_dir(&self, slot: Slot) -> PathBuf {
        self.root.join(format!("slot-{}", slot.letter()))
    }

    /// Path of the image file of `role` in `slot`.
    pub fn image_path(&self, slot: Slot, role: Role) -> PathBuf {
        self.slot_dir(slot).join(format!("{}.img", role.name()))
    }

    fn read_copy(&self, copy: usize) -> Result<Option<SlotTable>, UpdateError> {
        match read_bounded(&self.table_path(copy), TABLE_LEN as u64) {
            Ok(Some(bytes)) => Ok(SlotTable::decode(&bytes).ok()),
            Ok(None) => Ok(None),
            Err(e) if e.kind() == io::ErrorKind::InvalidData => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn write_copy(&self, copy: usize, bytes: &[u8; TABLE_LEN]) -> Result<(), UpdateError> {
        // In place, without rename: this is how the table is written on a FAT start
        // partition. A torn write damages this copy only; the other one stays valid.
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.table_path(copy))?;
        file.write_all(bytes)?;
        file.set_len(TABLE_LEN as u64)?;
        file.sync_all()?;
        sync_dir(&self.root)?;
        Ok(())
    }

    /// The current slot table. [`UpdateError::BootControl`] when both copies are invalid.
    pub fn table(&self) -> Result<SlotTable, UpdateError> {
        SlotTable::newest([self.read_copy(0)?, self.read_copy(1)?]).ok_or(UpdateError::BootControl)
    }

    fn store(&self, mut table: SlotTable) -> Result<(), UpdateError> {
        let copies = [self.read_copy(0)?, self.read_copy(1)?];
        let current = SlotTable::newest(copies).ok_or(UpdateError::BootControl)?;
        table.seq = current.seq;
        if table == current {
            return Ok(());
        }
        table.seq = current.seq.checked_add(1).ok_or(UpdateError::BootControl)?;
        // Overwrite the copy that does not hold the current table.
        let target = usize::from(copies[0] == Some(current));
        self.write_copy(target, &table.encode())
    }

    fn change(&self, edit: impl FnOnce(&mut SlotTable)) -> Result<(), UpdateError> {
        let mut table = self.table()?;
        edit(&mut table);
        self.store(table)
    }

    /// Stand-in for the boot code: takes the start decision ([`select`]), stores it and returns
    /// the slot that "starts". [`UpdateError::NoBootableSlot`] when nothing can start.
    pub fn simulate_boot(&self) -> Result<Slot, UpdateError> {
        let (slot, next) = select(&self.table()?).ok_or(UpdateError::NoBootableSlot)?;
        self.store(next)?;
        Ok(slot)
    }

    /// The verity root hash recorded for `slot` when it was made bootable.
    pub fn verity_root(&self, slot: Slot) -> Result<Option<[u8; 32]>, UpdateError> {
        let bytes = read_bounded(&self.slot_dir(slot).join("verity"), 128)?.unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        let mut root = [0u8; 32];
        Ok(hex::decode_to_slice(text.trim(), &mut root)
            .ok()
            .map(|()| root))
    }
}

impl BootControl for FileBootControl {
    fn active_slot(&self) -> Result<Slot, UpdateError> {
        self.table()?.last_booted.ok_or(UpdateError::BootControl)
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
        let table = self.table()?;
        // The running slot may only be given up while it is a trial: the default slot is
        // then the other one, and the next start goes there.
        if table.last_booted == Some(slot) && table.default == slot {
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
        let table = self.table()?;
        let in_use = table.last_booted == Some(slot) || table.slot(slot).bootable;
        if in_use || !self.roles.contains(&role) {
            return Err(UpdateError::InvalidArgument);
        }
        let dir = self.slot_dir(slot);
        fs::create_dir_all(&dir)?;
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.image_path(slot, role))?;
        file.set_len(size)?;
        Ok(Box::new(FileSlotWriter {
            file,
            dir,
            remaining: size,
        }))
    }

    fn open_reader(&self, slot: Slot, role: Role) -> Result<Box<dyn Read>, UpdateError> {
        Ok(Box::new(File::open(self.image_path(slot, role))?))
    }

    fn mark_bootable(&mut self, slot: Slot, meta: &SlotMeta) -> Result<(), UpdateError> {
        if self.active_slot()? == slot {
            return Err(UpdateError::InvalidArgument);
        }
        let root = meta
            .verity_root
            .map_or_else(|| "-".to_string(), hex::encode);
        write_atomic(
            &self.slot_dir(slot).join("verity"),
            format!("{root}\n").as_bytes(),
        )?;
        self.change(|table| {
            *table.slot_mut(slot) = SlotFlags {
                bootable: true,
                successful: false,
            };
        })
    }

    fn set_next_boot_once(&mut self, slot: Slot, tries: u8) -> Result<(), UpdateError> {
        if tries == 0 || !self.table()?.slot(slot).bootable {
            return Err(UpdateError::InvalidArgument);
        }
        self.change(|table| table.trial = Some((slot, tries)))
    }

    fn mark_successful(&mut self) -> Result<(), UpdateError> {
        let active = self.active_slot()?;
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
#[path = "filectl_tests.rs"]
mod tests;
