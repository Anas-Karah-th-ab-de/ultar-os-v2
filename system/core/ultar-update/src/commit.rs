//! After the restart: confirm the new release, or record that the device fell back.

use crate::bootctl::{BootControl, Slot};
use crate::engine::Engine;
use crate::error::UpdateError;
use crate::fault::Step;
use crate::health::HealthCheck;
use crate::history::HistoryKind;
use crate::journal::Journal;
use crate::version::Version;

/// What [`Engine::resume_after_boot`] found and did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootOutcome {
    /// Nothing was pending.
    Nothing,
    /// A release is installed and waits for the restart.
    RebootPending,
    /// The new release runs, passed its checks and is now the system.
    Committed {
        /// The confirmed version (`None` when its manifest could not be read any more).
        version: Option<Version>,
    },
    /// The new release did not start; the previous system is running.
    RolledBack,
    /// The new release started but failed its health checks; it was given up and a restart
    /// returns to the previous system.
    HealthFailed,
}

impl<B: BootControl> Engine<B> {
    /// To be called once after every system start, when the services are up.
    ///
    /// Confirms a new release only when `health` passes; the rollback floor is raised only
    /// here. Safe to call again after a power loss at any point.
    pub fn resume_after_boot(
        &mut self,
        health: &dyn HealthCheck,
    ) -> Result<BootOutcome, UpdateError> {
        let active = self.boot.active_slot()?;
        let state = self.boot.slot_state(active)?;
        if !state.bootable {
            // The running slot was already given up (failed health check) and the device has
            // not been restarted since: nothing to judge again, the restart is still due.
            return Ok(BootOutcome::HealthFailed);
        }
        let confirmed = state.successful;
        match self.journal()? {
            Journal::Trial { slot, .. } if slot != active => self.trial_elsewhere(slot),
            Journal::Trial { .. } | Journal::Idle if !confirmed => {
                match health.check() {
                    Ok(()) => self.commit(active),
                    // The service is stopping while it waits: no verdict either way; the next
                    // start judges again.
                    Err(UpdateError::Interrupted) => Err(UpdateError::Interrupted),
                    Err(_) => self.give_up_running_slot(active),
                }
            }
            Journal::Trial { .. } => self.commit(active),
            Journal::Writing { .. } => {
                self.recover_interrupted_write()?;
                Ok(BootOutcome::Nothing)
            }
            Journal::Idle => Ok(BootOutcome::Nothing),
        }
    }

    /// The journal expects a trial of `slot`, but another slot is running.
    fn trial_elsewhere(&mut self, slot: Slot) -> Result<BootOutcome, UpdateError> {
        let state = self.boot.slot_state(slot)?;
        if state.bootable {
            if state.trial_tries.is_none() {
                // Power loss between "journal: trial" and arming the start: finish it.
                self.env.control.reached(Step::ArmTrial)?;
                self.boot
                    .set_next_boot_once(slot, self.config.trial_tries)?;
            }
            return Ok(BootOutcome::RebootPending);
        }
        let version = self.slot_manifest(slot).map(|m| m.version);
        self.reject_slot(slot)?;
        Journal::Idle.store(&self.journal_path())?;
        self.log(
            HistoryKind::RolledBack,
            version,
            Some(UpdateError::BootFailed.key()),
        );
        Ok(BootOutcome::RolledBack)
    }

    fn give_up_running_slot(&mut self, active: Slot) -> Result<BootOutcome, UpdateError> {
        let version = self.slot_manifest(active).map(|m| m.version);
        self.reject_slot(active)?;
        self.boot.mark_unbootable(active)?;
        Journal::Idle.store(&self.journal_path())?;
        self.log(
            HistoryKind::RolledBack,
            version,
            Some(UpdateError::Unhealthy.key()),
        );
        Ok(BootOutcome::HealthFailed)
    }

    /// Makes the running slot the system. Every step is idempotent.
    fn commit(&mut self, active: Slot) -> Result<BootOutcome, UpdateError> {
        let control = self.env.control.clone();
        control.reached(Step::CommitSuccessful)?;
        self.boot.mark_successful()?;
        let installed = self.slot_manifest(active);
        if let Some(manifest) = &installed {
            control.reached(Step::CommitFloor)?;
            let index = manifest.rollback_index;
            self.floor.raise(index)?;
            self.boot.raise_firmware_rollback_index(index)?;
        }
        control.reached(Step::CommitRetireOther)?;
        let other = active.other();
        if self.slot_index(other) < self.rollback_floor()? {
            self.boot.mark_unbootable(other)?;
        }
        control.reached(Step::CommitStaging)?;
        self.staging.purge()?;
        control.reached(Step::JournalIdle)?;
        let was_pending = self.journal()? != Journal::Idle;
        self.clear_rejected()?;
        Journal::Idle.store(&self.journal_path())?;
        let version = installed.map(|m| m.version);
        if was_pending {
            self.log(HistoryKind::Installed, version, None);
        }
        Ok(BootOutcome::Committed { version })
    }

    /// Manual "start the previous version": the other slot becomes the default for the next
    /// start. Refused when it is not bootable, never proved itself, is below the rollback
    /// floor, or an installation is pending.
    pub fn boot_previous(&mut self) -> Result<Option<Version>, UpdateError> {
        if self.journal()? != Journal::Idle {
            return Err(UpdateError::Busy);
        }
        if !self.previous_startable()? {
            return Err(UpdateError::PreviousUnavailable);
        }
        let active = self.boot.active_slot()?;
        let other = active.other();
        self.boot.set_default(other)?;
        // The administrator leaves the running release: the device must not bring it back
        // by itself (a different release, or a manual install, still can).
        self.reject_slot(active)?;
        let version = self.slot_facts(other).map(|(version, _)| version);
        self.log(HistoryKind::PreviousSelected, version, None);
        Ok(version)
    }

    /// Drops whatever is pending: a release waiting for its restart is disarmed (its slot
    /// becomes unbootable), and the staging area is emptied. A running, unconfirmed release
    /// is not touched – that is decided by [`Engine::resume_after_boot`].
    pub fn cancel_pending(&mut self) -> Result<(), UpdateError> {
        let active = self.boot.active_slot()?;
        match self.journal()? {
            Journal::Trial { slot, .. } if slot != active => {
                self.boot.mark_unbootable(slot)?;
                Journal::Idle.store(&self.journal_path())?;
            }
            Journal::Trial { .. } => return Err(UpdateError::Busy),
            Journal::Writing { .. } => self.recover_interrupted_write()?,
            Journal::Idle => {}
        }
        self.staging.purge()?;
        self.log(HistoryKind::Cancelled, None, None);
        Ok(())
    }
}
