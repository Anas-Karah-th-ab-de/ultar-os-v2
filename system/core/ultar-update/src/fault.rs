//! Checkpoints: the places where an operation may stop.
//!
//! The updater is written "crash only": when an operation stops at a checkpoint – because the
//! user cancelled, or because a test injects a power loss – it returns at once **without any
//! clean-up**. Everything must be recoverable from what is on disk, exactly as after a real
//! power loss. Cancelling and fault injection therefore use the same mechanism.

use crate::error::UpdateError;
use crate::manifest::Role;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// A point in an operation, reported *before* the named action is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Before a chunk (64 KiB) of a download or bundle copy is stored.
    DownloadChunk,
    /// Before the flushed length of the download is recorded.
    DownloadFlushed,
    /// Before the finished download gets its final name.
    StageRename,
    /// Before the completion marker of the staged bundle is written.
    StageMarker,
    /// Before the journal records "writing".
    JournalWriting,
    /// Before the target slot is marked unbootable.
    SlotUnbootable,
    /// Before the stored manifest of the target slot is removed.
    SlotManifestRemove,
    /// Before a chunk (64 KiB) of a payload is written to the slot.
    PayloadChunk(Role),
    /// Before a written payload is flushed.
    PayloadFlush(Role),
    /// Before a payload is read back from the slot.
    ReadBack(Role),
    /// Before the signed manifest of the slot is stored.
    SlotManifestStore,
    /// Before the slot is marked bootable.
    SlotBootable,
    /// Before the journal records "trial".
    JournalTrial,
    /// Before the one-time start of the slot is armed.
    ArmTrial,
    /// Before the running slot is marked successful.
    CommitSuccessful,
    /// Before the rollback floor is raised.
    CommitFloor,
    /// Before an outdated other slot is marked unbootable.
    CommitRetireOther,
    /// Before the staging area is removed after a commit.
    CommitStaging,
    /// Before the journal returns to "idle".
    JournalIdle,
}

impl Step {
    /// True for steps at which a user's cancel request is honoured. The commit after a
    /// successful start is never cancelled.
    pub fn cancellable(self) -> bool {
        !matches!(
            self,
            Step::CommitSuccessful
                | Step::CommitFloor
                | Step::CommitRetireOther
                | Step::CommitStaging
                | Step::JournalIdle
        )
    }
}

/// Decides at every checkpoint whether the operation goes on.
pub trait Checkpoint: Send + Sync {
    /// `Ok` to continue; an error stops the operation at once (no clean-up runs).
    fn reached(&self, step: Step) -> Result<(), UpdateError>;
}

/// The production checkpoint: stops with [`UpdateError::Cancelled`] once cancelled.
#[derive(Debug, Default)]
pub struct CancelFlag(AtomicBool);

impl CancelFlag {
    /// A flag that is not set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks the running operation to stop at its next checkpoint.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Clears the request (before a new operation starts).
    pub fn reset(&self) {
        self.0.store(false, Ordering::SeqCst);
    }

    /// True while a cancel request is pending.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

impl Checkpoint for CancelFlag {
    fn reached(&self, step: Step) -> Result<(), UpdateError> {
        if step.cancellable() && self.is_cancelled() {
            Err(UpdateError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Test instrument: lets a fixed number of checkpoints pass, then stops every further one with
/// [`UpdateError::Interrupted`] – a simulated power loss. Also counts the checkpoints it saw.
#[derive(Debug)]
pub struct InterruptAfter {
    allowed: AtomicU64,
    seen: AtomicU64,
}

impl InterruptAfter {
    /// Lets `allowed` checkpoints pass.
    pub fn new(allowed: u64) -> Self {
        Self {
            allowed: AtomicU64::new(allowed),
            seen: AtomicU64::new(0),
        }
    }

    /// Never interrupts; only counts.
    pub fn never() -> Self {
        Self::new(u64::MAX)
    }

    /// Checkpoints reached so far (including refused ones).
    pub fn seen(&self) -> u64 {
        self.seen.load(Ordering::SeqCst)
    }
}

impl Checkpoint for InterruptAfter {
    fn reached(&self, _step: Step) -> Result<(), UpdateError> {
        let seen = self.seen.fetch_add(1, Ordering::SeqCst);
        if seen >= self.allowed.load(Ordering::SeqCst) {
            Err(UpdateError::Interrupted)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_flag_spares_the_commit() {
        let flag = CancelFlag::new();
        assert!(flag.reached(Step::DownloadChunk).is_ok());
        flag.cancel();
        assert!(matches!(
            flag.reached(Step::PayloadChunk(Role::System)),
            Err(UpdateError::Cancelled)
        ));
        assert!(flag.reached(Step::CommitFloor).is_ok());
        assert!(flag.reached(Step::JournalIdle).is_ok());
        flag.reset();
        assert!(flag.reached(Step::ArmTrial).is_ok());
    }

    #[test]
    fn interrupt_after_counts() {
        let fault = InterruptAfter::new(2);
        assert!(fault.reached(Step::DownloadChunk).is_ok());
        assert!(fault.reached(Step::DownloadChunk).is_ok());
        assert!(matches!(
            fault.reached(Step::StageMarker),
            Err(UpdateError::Interrupted)
        ));
        assert!(fault.reached(Step::StageMarker).is_err());
        assert_eq!(fault.seen(), 4);
        let never = InterruptAfter::never();
        assert!(never.reached(Step::ArmTrial).is_ok());
    }
}
