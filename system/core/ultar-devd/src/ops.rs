//! Manual operations for the settings panel "Drivers & hardware".

use crate::error::DevdError;
use crate::fsm::{Phase, Signal};
use crate::manager::DeviceManager;
use crate::probe::RunEnd;
use crate::traits::{CapabilitySink, ChoiceStore, DriverLoader, HealthCheck};
use crate::types::{Capability, DriverKey, FailureReason, ProbeReport, QuarantineEntry};
use ultar_driverpkg::{Candidate, DeviceDescriptor};

/// A driver that could be used for a device instead of the active one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlternativeView {
    /// The driver.
    pub driver: Candidate,
    /// Set when the driver is quarantined for this device, with the reason.
    pub quarantined: Option<FailureReason>,
}

/// One row of the device list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceView {
    /// The device.
    pub device: DeviceDescriptor,
    /// Its phase.
    pub phase: Phase,
    /// The active driver, if any.
    pub active: Option<Candidate>,
    /// Capabilities the active driver provides.
    pub capabilities: Vec<Capability>,
    /// Other fitting drivers, best first.
    pub alternatives: Vec<AlternativeView>,
    /// The driver a roll-back would return to.
    pub previous: Option<DriverKey>,
}

/// Which quarantine entries to clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuarantineFilter {
    /// Every entry.
    All,
    /// Every entry of the device at this address.
    Device(String),
    /// One driver on the device at this address.
    Driver {
        /// Address of the device.
        device: String,
        /// The driver to release.
        driver: DriverKey,
    },
}

impl<L: DriverLoader, H: HealthCheck, S: ChoiceStore, K: CapabilitySink> DeviceManager<L, H, S, K> {
    /// Lists all present devices with active driver, alternatives and state, ordered by address.
    pub fn devices(&self) -> Vec<DeviceView> {
        self.devices
            .values()
            .map(|record| {
                let identity = record.device.identity();
                let active = record.active.as_ref().map(DriverKey::of);
                let alternatives = self
                    .candidates_for(&record.device)
                    .into_iter()
                    .filter(|c| Some(DriverKey::of(c)) != active)
                    .map(|driver| {
                        let entry = self.state.quarantined(&identity, &DriverKey::of(&driver));
                        AlternativeView {
                            quarantined: entry.map(|e| e.reason),
                            driver,
                        }
                    })
                    .collect();
                DeviceView {
                    device: record.device.clone(),
                    phase: record.phase,
                    active: record.active.clone(),
                    capabilities: record.capabilities.clone(),
                    alternatives,
                    previous: self.state.previous.get(&identity).cloned(),
                }
            })
            .collect()
    }

    /// Switches a device to another driver.
    ///
    /// If the requested driver does not work it is quarantined and the previous driver is
    /// restored ([`ProbeReport::restored`]); should that fail too, the remaining candidates
    /// are tried. On success the replaced driver becomes the roll-back target.
    pub fn switch_driver(
        &mut self,
        address: &str,
        driver: &DriverKey,
    ) -> Result<ProbeReport, DevdError> {
        let device = self.device(address)?;
        let identity = device.identity();
        let all = self.candidates_for(&device);
        let Some(target) = all.iter().find(|c| &DriverKey::of(c) == driver).cloned() else {
            return Err(DevdError::NotACandidate {
                device: address.to_string(),
                driver: driver.clone(),
            });
        };
        if self.state.quarantined(&identity, driver).is_some() {
            return Err(DevdError::Quarantined {
                device: address.to_string(),
                driver: driver.clone(),
            });
        }
        let mut report = ProbeReport::new(address);
        let old = self.begin_switch(address, driver, &mut report)?;
        let snapshot = self.state.quarantine.clone();
        match self.run(address, std::slice::from_ref(&target), &mut report)? {
            RunEnd::Active => {
                if let Some(old) = &old {
                    self.state.previous.insert(identity, DriverKey::of(old));
                }
            }
            RunEnd::Gone => self.finish(address, RunEnd::Gone, snapshot, &mut report)?,
            RunEnd::Exhausted => {
                let order = self.fallback_order(&identity, old.as_ref(), &all, driver);
                let end = self.run(address, &order, &mut report)?;
                report.restored =
                    end == RunEnd::Active && report.active == old.as_ref().map(DriverKey::of);
                self.finish(address, end, snapshot, &mut report)?;
            }
        }
        self.persist(&mut report);
        Ok(report)
    }

    /// Moves the device into `Probing` for a manual switch and unloads the active driver.
    fn begin_switch(
        &mut self,
        address: &str,
        driver: &DriverKey,
        report: &mut ProbeReport,
    ) -> Result<Option<Candidate>, DevdError> {
        let record = self
            .devices
            .get(address)
            .ok_or_else(|| DevdError::UnknownDevice(address.to_string()))?;
        match record.phase {
            Phase::Active => {
                if record.active.as_ref().map(DriverKey::of).as_ref() == Some(driver) {
                    return Err(DevdError::AlreadyActive(driver.clone()));
                }
                self.step(address, Signal::SwitchRequested, report)?;
                Ok(self.deactivate(address))
            }
            Phase::NoDriver | Phase::Failed => {
                self.step(address, Signal::Rescan, report)?;
                self.step(address, Signal::CandidatesFound, report)?;
                Ok(None)
            }
            phase => Err(DevdError::WrongPhase {
                device: address.to_string(),
                phase,
            }),
        }
    }

    /// After a failed switch: the previous driver first, then the other usable candidates.
    fn fallback_order(
        &self,
        identity: &str,
        old: Option<&Candidate>,
        all: &[Candidate],
        failed: &DriverKey,
    ) -> Vec<Candidate> {
        let old_key = old.map(DriverKey::of);
        let rest = all.iter().filter(|c| {
            let key = DriverKey::of(c);
            &key != failed
                && Some(&key) != old_key.as_ref()
                && self.state.quarantined(identity, &key).is_none()
        });
        old.into_iter().chain(rest).cloned().collect()
    }

    /// Returns to the driver that was active before the last successful switch.
    pub fn roll_back(&mut self, address: &str) -> Result<ProbeReport, DevdError> {
        let identity = self.device(address)?.identity();
        let previous = self
            .state
            .previous
            .get(&identity)
            .cloned()
            .ok_or_else(|| DevdError::NothingToRollBack(address.to_string()))?;
        self.switch_driver(address, &previous)
    }

    /// Releases drivers from quarantine and returns how many entries were removed.
    /// Released drivers are tried again at the next probe ([`DeviceManager::rescan`], next boot).
    pub fn clear_quarantine(&mut self, filter: &QuarantineFilter) -> Result<usize, DevdError> {
        let before = self.state.quarantine.len();
        match filter {
            QuarantineFilter::All => self.state.quarantine.clear(),
            QuarantineFilter::Device(address) => {
                let identity = self.device(address)?.identity();
                self.state
                    .quarantine
                    .retain(|entry: &QuarantineEntry| entry.device != identity);
            }
            QuarantineFilter::Driver { device, driver } => {
                let identity = self.device(device)?.identity();
                self.state.release(&identity, driver);
            }
        }
        let mut report = ProbeReport::new("");
        self.persist(&mut report);
        Ok(before - self.state.quarantine.len())
    }

    /// Searches again for a device that has no working driver (new index, cleared quarantine).
    pub fn rescan(&mut self, address: &str) -> Result<ProbeReport, DevdError> {
        let phase = self
            .phase(address)
            .ok_or_else(|| DevdError::UnknownDevice(address.to_string()))?;
        if !matches!(phase, Phase::NoDriver | Phase::Failed) {
            return Err(DevdError::WrongPhase {
                device: address.to_string(),
                phase,
            });
        }
        let mut report = ProbeReport::new(address);
        self.step(address, Signal::Rescan, &mut report)?;
        self.probe(address, &mut report)?;
        self.persist(&mut report);
        Ok(report)
    }
}
