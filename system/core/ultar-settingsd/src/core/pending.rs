//! Confirm or revert: keeping and taking back provisional values, the
//! clock tick, and the recovery after a restart.

use super::{slot_of, Core, Event};
use crate::error::SettingsError;
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::settings::PendingOutcome;

impl Core {
    fn pending_selection(&self, key: Option<&str>) -> Result<Vec<String>, SettingsError> {
        match key {
            None => Ok(self.pending.keys().cloned().collect()),
            Some(key) if self.pending.contains_key(key) => Ok(vec![key.to_owned()]),
            Some(key) => {
                self.registry.resolve(key)?;
                Err(SettingsError::NotPending)
            }
        }
    }

    /// Keeps the provisional value of `key`, or of every pending key.
    /// Returns how many changes were kept.
    pub fn confirm(&mut self, key: Option<&str>) -> Result<u32, SettingsError> {
        // a change whose deadline passed is taken back first, even when the tick has not run
        // yet: a late "Keep" never keeps it
        self.revert_due()?;
        let keys = self.pending_selection(key)?;
        let registry = Arc::clone(&self.registry);
        let mut kept = 0;
        for key in &keys {
            let (Some(pending), Ok(found)) = (self.pending.remove(key), registry.resolve(key))
            else {
                continue;
            };
            self.ensure_slot(found.def.scope);
            let slot = slot_of(found.def.scope, &self.combo);
            match pending.target {
                Some(value) => self.store.set(slot, key, value),
                None => self.store.remove(slot, key),
            };
            let value = self.stored_or_default(&found);
            self.events.push(Event::Resolved {
                key: key.clone(),
                outcome: PendingOutcome::Kept,
                value,
            });
            kept += 1;
        }
        self.store.journal_remove(&keys)?;
        Ok(kept)
    }

    /// Takes the provisional value of `key`, or of every pending key, back
    /// now. Returns how many changes were taken back.
    pub fn revert(&mut self, key: Option<&str>) -> Result<u32, SettingsError> {
        let keys = self.pending_selection(key)?;
        self.revert_keys(&keys, PendingOutcome::Reverted)
    }

    /// Restores the stored value of each key and announces it. All keys are
    /// processed even when one fails; the first failure is returned.
    pub(super) fn revert_keys(
        &mut self,
        keys: &[String],
        outcome: PendingOutcome,
    ) -> Result<u32, SettingsError> {
        let registry = Arc::clone(&self.registry);
        let mut failure = None;
        let mut reverted = 0;
        for key in keys {
            self.pending.remove(key);
            let Ok(found) = registry.resolve(key) else {
                continue;
            };
            if self.check_instance(&found).is_err() {
                continue;
            }
            let value = self.stored_or_default(&found);
            if let Err(error) = self.apply(&found, &value, false) {
                self.stats.apply_failures += 1;
                failure = failure.or(Some(error));
            }
            self.events.push(Event::Changed {
                key: key.clone(),
                value: value.clone(),
            });
            self.events.push(Event::Resolved {
                key: key.clone(),
                outcome,
                value,
            });
            reverted += 1;
        }
        if let Err(error) = self.store.journal_remove(keys) {
            failure = failure.or(Some(error.into()));
        }
        failure.map_or(Ok(reverted), Err)
    }

    /// Takes back what was pending when the service last stopped.
    pub(super) fn revert_interrupted(&mut self) -> Result<(), SettingsError> {
        let keys: Vec<String> = self.store.journal().map(str::to_owned).collect();
        if keys.is_empty() {
            return Ok(());
        }
        match self.revert_keys(&keys, PendingOutcome::RevertedAtStart) {
            // An applier that fails at start must not keep the service
            // from starting; the failure is counted.
            Ok(_) | Err(SettingsError::Apply(_)) => Ok(()),
            // The journal file could not be rewritten (disk full, read-only):
            // the values are taken back all the same; the entries stay on
            // disk and are handled again at the next start, which is harmless.
            Err(SettingsError::Store(_)) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Does what is due at the current time: takes back provisional values
    /// whose deadline passed and writes waiting changes to disk. Everything
    /// due is done even when one step fails; the first failure is returned.
    pub fn tick(&mut self) -> Result<(), SettingsError> {
        let reverted = self.revert_due();
        let written = self.store.tick();
        reverted?;
        written?;
        Ok(())
    }

    /// Takes back the provisional values whose deadline passed.
    fn revert_due(&mut self) -> Result<u32, SettingsError> {
        let now = self.clock.now();
        let expired: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.deadline <= now)
            .map(|(key, _)| key.clone())
            .collect();
        if expired.is_empty() {
            return Ok(0);
        }
        self.revert_keys(&expired, PendingOutcome::TimedOut)
    }

    /// When [`Core::tick`] has to run next, if anything is waiting.
    pub fn next_deadline(&self) -> Option<Duration> {
        self.pending
            .values()
            .map(|pending| pending.deadline)
            .chain(self.store.next_deadline())
            .min()
    }

    /// Keys with a provisional value, sorted.
    pub fn pending_keys(&self) -> Vec<String> {
        self.pending.keys().cloned().collect()
    }
}
