//! The start decision with its durable write, and the start loop.

use crate::disk::{Disk, DiskError, Parts, MAX_BLOCK};
use ultar_slots::layout::TABLE_COPY_OFFSETS;
use ultar_slots::{plan_store, select, Slot, SlotTable, TABLE_LEN};

/// Why the loader goes to the rescue path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rescue {
    /// Neither table copy is valid.
    NoTable,
    /// The table allows no slot.
    NoBootableSlot,
    /// The chosen slot (not a trial) has no usable kernel.
    KernelUnusable(Slot),
    /// Too many failed attempts in one start (defensive bound).
    TooManyAttempts,
}

/// What to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Start {
    /// The slot.
    pub slot: Slot,
    /// A counted attempt of a new slot (`ultar.trial=1`).
    pub trial: bool,
    /// The decision reached the disk (written and flushed). When it did not, a trial is never
    /// started: an attempt that is not counted could repeat forever.
    pub stored: bool,
}

/// Upper bound of start attempts in one run (a trial has at most 255 tries; in practice 3).
pub const MAX_ATTEMPTS: usize = 8;

fn copy_lba(parts: &Parts, copy: usize, block: usize) -> u64 {
    parts.bootctl.first_lba + TABLE_COPY_OFFSETS[copy] / block as u64
}

/// Reads both table copies; an unreadable or invalid copy is `None`.
pub fn read_copies<D: Disk>(
    disk: &mut D,
    parts: &Parts,
    block: &mut [u8],
) -> [Option<SlotTable>; 2] {
    let bs = disk.block_size().min(MAX_BLOCK);
    let mut out = [None, None];
    for (copy, slot) in out.iter_mut().enumerate() {
        if disk
            .read(copy_lba(parts, copy, bs), &mut block[..bs])
            .is_ok()
        {
            *slot = SlotTable::decode(&block[..TABLE_LEN]).ok();
        }
    }
    out
}

fn store<D: Disk>(
    disk: &mut D,
    parts: &Parts,
    copy: usize,
    table: &SlotTable,
    block: &mut [u8],
) -> Result<(), DiskError> {
    let bs = disk.block_size().min(MAX_BLOCK);
    block[..bs].fill(0);
    block[..TABLE_LEN].copy_from_slice(&table.encode());
    disk.write(copy_lba(parts, copy, bs), &block[..bs])?;
    disk.flush()
}

/// Takes the start decision and stores it (write + flush) before returning.
/// `block` is a buffer of at least one block.
pub fn decide<D: Disk>(disk: &mut D, parts: &Parts, block: &mut [u8]) -> Result<Start, Rescue> {
    let copies = read_copies(disk, parts, block);
    let current = SlotTable::newest(copies).ok_or(Rescue::NoTable)?;
    let (slot, next) = select(&current).ok_or(Rescue::NoBootableSlot)?;
    let trial = next.trial.is_some_and(|(s, _)| s == slot);
    let stored = match plan_store(copies, next) {
        Ok(None) => true,
        Ok(Some((copy, table))) => store(disk, parts, copy, &table, block).is_ok(),
        Err(_) => false,
    };
    if stored || !trial {
        return Ok(Start {
            slot,
            trial,
            stored,
        });
    }
    // The counted attempt could not be recorded: start as if no trial were pending.
    let mut plain = current;
    plain.trial = None;
    let (slot, _) = select(&plain).ok_or(Rescue::NoBootableSlot)?;
    Ok(Start {
        slot,
        trial: false,
        stored: false,
    })
}

/// The start loop. `start` loads and starts the kernel of a slot and returns only when that
/// failed (bad image, firmware refused it, or the kernel returned). A failed trial costs its
/// attempt and the decision is taken again (next attempt, then the fallback); a failed
/// regular start ends in the rescue path.
pub fn run<D: Disk>(
    disk: &mut D,
    parts: &Parts,
    block: &mut [u8],
    start: &mut dyn FnMut(&mut D, Start),
) -> Rescue {
    for _ in 0..MAX_ATTEMPTS {
        let chosen = match decide(disk, parts, block) {
            Ok(s) => s,
            Err(r) => return r,
        };
        start(disk, chosen);
        if !chosen.trial {
            return Rescue::KernelUnusable(chosen.slot);
        }
    }
    Rescue::TooManyAttempts
}

#[cfg(test)]
#[path = "boot_tests.rs"]
mod tests;
