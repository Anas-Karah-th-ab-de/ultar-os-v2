//! Installing a staged bundle into the slot that is not running.
//!
//! Order of the steps and why it is safe at every point: `docs/SYSTEM-UPDATE.md` §6. The rule
//! that carries everything: the target slot is unbootable from before its first byte is
//! written until after its last byte has been read back and compared.

use crate::bootctl::{BootControl, Slot, SlotMeta};
use crate::bundle::Header;
use crate::engine::Engine;
use crate::error::UpdateError;
use crate::fault::Step;
use crate::fsutil::write_atomic;
use crate::history::HistoryKind;
use crate::journal::Journal;
use crate::manifest::{Payload, Role};
use crate::segfile::SegFile;
use crate::stage::{StagedInfo, CHUNK};
use crate::verify::VerifiedManifest;
use crate::version::Version;
use sha2::{Digest, Sha256};
use std::io::Read;

/// Which part of an installation is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyStage {
    /// A payload is written to the slot.
    Write(Role),
    /// A payload is read back and compared.
    ReadBack(Role),
}

/// Result of a successful installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    /// The installed version.
    pub version: Version,
    /// The slot it is in; the next start goes there once.
    pub slot: Slot,
}

/// Progress callback of an installation: stage, bytes done, bytes in total (all payloads).
pub type ApplyProgress<'a> = &'a mut dyn FnMut(ApplyStage, u64, u64);

impl<B: BootControl> Engine<B> {
    /// Installs the staged bundle into the other slot and arms its one-time start.
    ///
    /// Everything is verified again first (marker, size, signature, admission). On any failure
    /// the running system is untouched and the target slot stays unbootable. After
    /// [`UpdateError::Interrupted`] nothing is tidied up (that is the power-loss case; the next
    /// [`Engine::open`] recovers); after every other failure the journal returns to idle.
    pub fn apply(&mut self, progress: ApplyProgress<'_>) -> Result<Applied, UpdateError> {
        let outcome = self.apply_steps(progress);
        match &outcome {
            Ok(applied) => self.log(HistoryKind::Applied, Some(applied.version), None),
            Err(UpdateError::Interrupted) => {}
            Err(error) => {
                // Not staged / busy / refused before the journal moved: nothing to recover.
                let _ = self.recover_interrupted_write();
                if !matches!(error, UpdateError::Busy | UpdateError::NotStaged) {
                    self.log(HistoryKind::Failed, None, Some(error.key()));
                }
            }
        }
        outcome
    }

    fn apply_steps(&mut self, progress: ApplyProgress<'_>) -> Result<Applied, UpdateError> {
        let active = self.boot.active_slot()?;
        let in_trial = !self.boot.slot_state(active)?.successful;
        if in_trial || self.journal()? != Journal::Idle {
            return Err(UpdateError::Busy);
        }
        let (mut file, verified) = self.open_verified_stage()?;
        let target = active.other();
        let control = self.env.control.clone();
        control.reached(Step::JournalWriting)?;
        Journal::Writing { slot: target }.store(&self.journal_path())?;
        control.reached(Step::SlotUnbootable)?;
        self.boot.mark_unbootable(target)?;
        control.reached(Step::SlotManifestRemove)?;
        self.forget_slot(target)?;
        let manifest_path = self.slot_manifest_path(target);

        let manifest = verified.manifest().clone();
        let total = manifest.payload_bytes() * 2;
        let mut done = 0u64;
        for payload in &manifest.payloads {
            self.write_payload(&mut file, target, payload, &mut done, total, progress)?;
        }
        for payload in &manifest.payloads {
            self.read_back(target, payload, &mut done, total, progress)?;
        }

        control.reached(Step::SlotManifestStore)?;
        write_atomic(&manifest_path, verified.signed())?;
        control.reached(Step::SlotBootable)?;
        let meta = SlotMeta {
            verity_root: manifest.payload(Role::System).and_then(|p| p.verity_root),
        };
        self.boot.mark_bootable(target, &meta)?;
        control.reached(Step::JournalTrial)?;
        let previous = self.current_version()?;
        Journal::Trial {
            slot: target,
            previous,
        }
        .store(&self.journal_path())?;
        control.reached(Step::ArmTrial)?;
        self.boot
            .set_next_boot_once(target, self.config.trial_tries)?;
        Ok(Applied {
            version: manifest.version,
            slot: target,
        })
    }

    /// Opens the staged bundle, positioned at the first payload byte, after verifying it
    /// again from scratch. A staged bundle that is refused now (expired meanwhile, key rotated
    /// out, floor raised, channel changed, damaged) is removed: it can never be installed and
    /// must not stand in the way of the next download.
    fn open_verified_stage(&mut self) -> Result<(SegFile, VerifiedManifest), UpdateError> {
        let (mut file, info) = self.staging.open_staged()?.ok_or(UpdateError::NotStaged)?;
        let checked = self.verify_stage(&mut file, &info);
        let keep = matches!(
            checked,
            Ok(_) | Err(UpdateError::Io(_) | UpdateError::StateCorrupt)
        );
        if !keep {
            self.staging.purge()?;
        }
        Ok((file, checked?))
    }

    fn verify_stage(
        &mut self,
        file: &mut SegFile,
        info: &StagedInfo,
    ) -> Result<VerifiedManifest, UpdateError> {
        let header = Header::read(file)?;
        // A bundle fetched from the network belongs to the channel it was fetched for; it is
        // installed only while that is still the channel of the policy.
        let channel = info.channel.as_deref();
        if channel.is_some_and(|c| c != self.policy().channel) {
            return Err(UpdateError::WrongChannel);
        }
        let verified = self.inspect(&header, channel, info.allow_downgrade)?;
        let size = header.len() + verified.manifest().payload_bytes();
        if verified.digest() != info.digest || size != info.size {
            return Err(UpdateError::NotStaged);
        }
        Ok(verified)
    }

    fn write_payload(
        &mut self,
        file: &mut SegFile,
        target: Slot,
        payload: &Payload,
        done: &mut u64,
        total: u64,
        progress: ApplyProgress<'_>,
    ) -> Result<(), UpdateError> {
        let control = self.env.control.clone();
        let mut writer = self.boot.open_writer(target, payload.role, payload.size)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; CHUNK];
        let mut left = payload.size;
        while left > 0 {
            control.reached(Step::PayloadChunk(payload.role))?;
            let want = usize::try_from(left).map_or(CHUNK, |l| l.min(CHUNK));
            file.read_exact(&mut buf[..want])?;
            hasher.update(&buf[..want]);
            writer.write_all(&buf[..want])?;
            left -= want as u64;
            *done += want as u64;
            progress(ApplyStage::Write(payload.role), *done, total);
        }
        control.reached(Step::PayloadFlush(payload.role))?;
        writer.finish()?;
        if <[u8; 32]>::from(hasher.finalize()) != payload.sha256 {
            // The staged copy changed after it was verified: it is worthless now.
            self.staging.purge()?;
            return Err(UpdateError::HashMismatch(payload.role));
        }
        Ok(())
    }

    fn read_back(
        &mut self,
        target: Slot,
        payload: &Payload,
        done: &mut u64,
        total: u64,
        progress: ApplyProgress<'_>,
    ) -> Result<(), UpdateError> {
        self.env.control.reached(Step::ReadBack(payload.role))?;
        let mut reader = self
            .boot
            .open_reader(target, payload.role)?
            .take(payload.size);
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; CHUNK];
        let mut seen = 0u64;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            seen += n as u64;
            *done += n as u64;
            progress(ApplyStage::ReadBack(payload.role), *done, total);
        }
        if seen != payload.size || <[u8; 32]>::from(hasher.finalize()) != payload.sha256 {
            return Err(UpdateError::ReadBackMismatch(payload.role));
        }
        Ok(())
    }

    /// After an interrupted slot write (journal says "writing"): makes sure the half-written
    /// slot cannot start and returns the journal to idle. The staged bundle is kept, so the
    /// installation can simply run again.
    pub(crate) fn recover_interrupted_write(&mut self) -> Result<(), UpdateError> {
        let Journal::Writing { slot } = self.journal()? else {
            return Ok(());
        };
        if self.boot.active_slot()? != slot {
            self.boot.mark_unbootable(slot)?;
            self.forget_slot(slot)?;
        }
        Journal::Idle.store(&self.journal_path())
    }
}
