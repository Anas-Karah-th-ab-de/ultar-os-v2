//! The probe engine: try candidates in order, quarantine failures, activate the first that works.

use crate::error::DevdError;
use crate::fsm::Signal;
use crate::manager::DeviceManager;
use crate::traits::{
    CapabilitySink, ChoiceStore, DriverLoader, HealthCheck, HealthVerdict, LoadError,
};
use crate::types::{
    Attempt, AttemptOutcome, Capability, CapabilityChange, ChangeKind, DriverKey, FailureReason,
    LoadFailure, ProbeReport, QuarantineEntry,
};
use ultar_driverpkg::{Candidate, DeviceDescriptor};

/// How a run over a list of candidates ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunEnd {
    /// A candidate works and is now active.
    Active,
    /// No candidate of the list works.
    Exhausted,
    /// The device disappeared during the run.
    Gone,
}

/// The result of trying one candidate.
enum Trial {
    Works(Vec<Capability>),
    Failed(FailureReason),
    Skipped,
    Gone,
}

impl<L: DriverLoader, H: HealthCheck, S: ChoiceStore, K: CapabilitySink> DeviceManager<L, H, S, K> {
    /// Loads one candidate and checks the device's health; a failed driver is unloaded again.
    fn trial(&mut self, device: &DeviceDescriptor, candidate: &Candidate) -> Trial {
        match self.loader.load(device, candidate) {
            Ok(()) => {}
            Err(LoadError::DeviceGone) => return Trial::Gone,
            Err(LoadError::Failed(LoadFailure::PackageMissing)) => return Trial::Skipped,
            Err(LoadError::Failed(failure)) => return Trial::Failed(FailureReason::Load(failure)),
        }
        match self.health.check(device, candidate) {
            HealthVerdict::Healthy(capabilities) => Trial::Works(capabilities),
            HealthVerdict::Unhealthy(failure) => {
                self.loader.unload(device, candidate);
                Trial::Failed(FailureReason::Health(failure))
            }
            HealthVerdict::DeviceGone => {
                self.loader.unload(device, candidate);
                Trial::Gone
            }
        }
    }

    /// Tries the candidates in order. The device must be in phase `Probing`.
    pub(crate) fn run(
        &mut self,
        address: &str,
        order: &[Candidate],
        report: &mut ProbeReport,
    ) -> Result<RunEnd, DevdError> {
        let device = self.device(address)?;
        let identity = device.identity();
        for candidate in order {
            let driver = DriverKey::of(candidate);
            let (outcome, end) = match self.trial(&device, candidate) {
                Trial::Works(capabilities) => {
                    self.state.release(&identity, &driver);
                    self.activate(address, candidate.clone(), capabilities, report)?;
                    (AttemptOutcome::Worked, Some(RunEnd::Active))
                }
                Trial::Failed(reason) => {
                    self.state
                        .quarantine_driver(&identity, driver.clone(), reason);
                    (AttemptOutcome::Failed(reason), None)
                }
                Trial::Skipped => (AttemptOutcome::Skipped, None),
                Trial::Gone => (AttemptOutcome::DeviceGone, Some(RunEnd::Gone)),
            };
            report.attempts.push(Attempt { driver, outcome });
            if let Some(end) = end {
                return Ok(end);
            }
        }
        Ok(RunEnd::Exhausted)
    }

    /// Full automatic search for a device in phase `Detected`.
    pub(crate) fn probe(
        &mut self,
        address: &str,
        report: &mut ProbeReport,
    ) -> Result<(), DevdError> {
        let device = self.device(address)?;
        let identity = device.identity();
        let all = self.candidates_for(&device);
        let remembered = self.state.choices.get(&identity).cloned();
        if all.is_empty() {
            self.step(address, Signal::NoCandidates, report)?;
            self.state.choices.remove(&identity);
            report.remembered_failed = remembered.is_some();
            return Ok(());
        }
        self.step(address, Signal::CandidatesFound, report)?;
        let snapshot = self.state.quarantine.clone();
        let (held, mut eligible): (Vec<Candidate>, Vec<Candidate>) =
            all.into_iter().partition(|c| {
                self.state
                    .quarantined(&identity, &DriverKey::of(c))
                    .is_some()
            });
        // The remembered driver goes first: the next boot loads it directly.
        if let Some(position) = eligible
            .iter()
            .position(|c| Some(DriverKey::of(c)) == remembered)
        {
            let first = eligible.remove(position);
            eligible.insert(0, first);
        }
        let mut end = self.run(address, &eligible, report)?;
        if end == RunEnd::Exhausted && self.config.last_resort_retry {
            end = self.run(address, &held, report)?;
        }
        if let Some(driver) = remembered {
            let direct = report.attempts.first();
            report.used_remembered =
                direct.is_some_and(|a| a.driver == driver && a.outcome == AttemptOutcome::Worked);
            report.remembered_failed = !report.used_remembered && end != RunEnd::Gone;
        }
        self.finish(address, end, snapshot, report)
    }

    /// Closes a probe: records exhaustion or handles a device that vanished meanwhile.
    pub(crate) fn finish(
        &mut self,
        address: &str,
        end: RunEnd,
        snapshot: Vec<QuarantineEntry>,
        report: &mut ProbeReport,
    ) -> Result<(), DevdError> {
        match end {
            RunEnd::Active => Ok(()),
            RunEnd::Exhausted => {
                let identity = self.device(address)?.identity();
                self.step(address, Signal::CandidatesExhausted, report)?;
                self.state.choices.remove(&identity);
                Ok(())
            }
            RunEnd::Gone => {
                // Failures seen while the device was disappearing say nothing about the drivers.
                self.state.quarantine = snapshot;
                self.remove(address, report)
            }
        }
    }

    /// Marks a candidate as the working driver, remembers it and announces its capabilities.
    fn activate(
        &mut self,
        address: &str,
        candidate: Candidate,
        capabilities: Vec<Capability>,
        report: &mut ProbeReport,
    ) -> Result<(), DevdError> {
        self.step(address, Signal::ProbeSucceeded, report)?;
        let driver = DriverKey::of(&candidate);
        let class = candidate.class;
        if let Some(record) = self.devices.get_mut(address) {
            self.state
                .choices
                .insert(record.device.identity(), driver.clone());
            record.active = Some(candidate);
            record.capabilities.clone_from(&capabilities);
        }
        report.active = Some(driver.clone());
        self.sink.emit(CapabilityChange {
            device: address.to_string(),
            driver,
            class,
            kind: ChangeKind::Gained,
            capabilities,
        });
        Ok(())
    }

    /// Unloads the active driver of a device and announces the lost capabilities.
    pub(crate) fn deactivate(&mut self, address: &str) -> Option<Candidate> {
        let record = self.devices.get_mut(address)?;
        let candidate = record.active.take()?;
        let capabilities = std::mem::take(&mut record.capabilities);
        let device = record.device.clone();
        self.loader.unload(&device, &candidate);
        self.sink.emit(CapabilityChange {
            device: address.to_string(),
            driver: DriverKey::of(&candidate),
            class: candidate.class,
            kind: ChangeKind::Lost,
            capabilities,
        });
        Some(candidate)
    }

    /// The descriptor of a present device.
    pub(crate) fn device(&self, address: &str) -> Result<DeviceDescriptor, DevdError> {
        self.devices
            .get(address)
            .map(|record| record.device.clone())
            .ok_or_else(|| DevdError::UnknownDevice(address.to_string()))
    }
}
