//! Boot control: the contract between the updater and whatever starts the system.
//!
//! The updater never decides by itself which slot starts; it only asks the boot control to
//! record intentions, and the boot code acts on them (see [`crate::slottable::select`]).

use crate::error::UpdateError;
use crate::manifest::Role;
use std::io::{Read, Write};

/// One of the two system slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    /// First slot.
    A,
    /// Second slot.
    B,
}

impl Slot {
    /// Both slots.
    pub const BOTH: [Slot; 2] = [Slot::A, Slot::B];

    /// The slot that is not this one.
    pub fn other(self) -> Slot {
        match self {
            Slot::A => Slot::B,
            Slot::B => Slot::A,
        }
    }

    /// 0 for A, 1 for B.
    pub fn index(self) -> usize {
        match self {
            Slot::A => 0,
            Slot::B => 1,
        }
    }

    /// The slot with this index.
    pub fn from_index(index: u8) -> Option<Slot> {
        match index {
            0 => Some(Slot::A),
            1 => Some(Slot::B),
            _ => None,
        }
    }

    /// `a` or `b`.
    pub fn letter(self) -> &'static str {
        match self {
            Slot::A => "a",
            Slot::B => "b",
        }
    }

    /// The slot with this letter.
    pub fn from_letter(letter: &str) -> Option<Slot> {
        Slot::BOTH.into_iter().find(|s| s.letter() == letter)
    }
}

/// What the boot control knows about a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotState {
    /// The boot code may start it.
    pub bootable: bool,
    /// It has started and passed its health checks at least once.
    pub successful: bool,
    /// `Some(n)`: it is to be started "once", `n` attempts are left.
    pub trial_tries: Option<u8>,
    /// It is the slot started when no trial is pending.
    pub is_default: bool,
}

/// Signed facts about a slot's content that the start chain needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotMeta {
    /// dm-verity root hash of the system image, when the release has one.
    pub verity_root: Option<[u8; 32]>,
}

/// A payload being written into a slot.
pub trait SlotWrite: Write {
    /// Flushes everything to the medium. Must be called before the content counts as written.
    fn finish(&mut self) -> Result<(), UpdateError>;
}

/// The operations the updater needs from the boot chain.
///
/// Every method that changes state must be durable when it returns and safe to repeat.
pub trait BootControl: Send {
    /// The slot the running system was started from.
    fn active_slot(&self) -> Result<Slot, UpdateError>;

    /// State of `slot`.
    fn slot_state(&self, slot: Slot) -> Result<SlotState, UpdateError>;

    /// The payload roles a slot of this device consists of.
    fn roles(&self) -> Vec<Role>;

    /// Forbids starting `slot` (and drops a pending trial of it). Refused for the running slot
    /// when it is the default; allowed for a running trial slot (the trial is given up).
    fn mark_unbootable(&mut self, slot: Slot) -> Result<(), UpdateError>;

    /// Opens the `role` area of `slot` for writing exactly `size` bytes from its start.
    fn open_writer(
        &mut self,
        slot: Slot,
        role: Role,
        size: u64,
    ) -> Result<Box<dyn SlotWrite>, UpdateError>;

    /// Opens the `role` area of `slot` for reading from its start.
    fn open_reader(&self, slot: Slot, role: Role) -> Result<Box<dyn Read>, UpdateError>;

    /// Declares the content of `slot` complete: it may be started. Not yet successful.
    fn mark_bootable(&mut self, slot: Slot, meta: &SlotMeta) -> Result<(), UpdateError>;

    /// The next start goes to `slot`, at most `tries` times, without changing the default.
    /// If it never reports success the boot code returns to the default slot by itself.
    fn set_next_boot_once(&mut self, slot: Slot, tries: u8) -> Result<(), UpdateError>;

    /// The running slot works: it becomes the default and the trial ends.
    fn mark_successful(&mut self) -> Result<(), UpdateError>;

    /// Makes `slot` the default for the following starts (manual "previous version").
    /// Refused unless the slot is bootable and has been successful before.
    fn set_default(&mut self, slot: Slot) -> Result<(), UpdateError>;

    /// A rollback index kept outside the file system (firmware variable, TPM), if the
    /// platform has one.
    fn firmware_rollback_index(&self) -> Result<Option<u64>, UpdateError> {
        Ok(None)
    }

    /// Raises the platform's rollback index, if it has one. Must never lower it.
    fn raise_firmware_rollback_index(&mut self, _index: u64) -> Result<(), UpdateError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_helpers() {
        assert_eq!(Slot::A.other(), Slot::B);
        assert_eq!(Slot::B.other(), Slot::A);
        for slot in Slot::BOTH {
            assert_eq!(Slot::from_index(slot.index() as u8), Some(slot));
            assert_eq!(Slot::from_letter(slot.letter()), Some(slot));
        }
        assert_eq!(Slot::from_index(2), None);
        assert_eq!(Slot::from_letter("c"), None);
    }
}
