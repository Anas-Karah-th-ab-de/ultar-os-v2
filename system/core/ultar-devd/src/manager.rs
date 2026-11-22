//! The device manager: owns the device table and reacts to device events.

use crate::error::DevdError;
use crate::fsm::{transition, Phase, Signal};
use crate::store::StoredState;
use crate::traits::{CapabilitySink, ChoiceStore, DriverLoader, HealthCheck, StoreError};
use crate::types::{Capability, DeviceEvent, ProbeReport, QuarantineEntry};
use std::collections::BTreeMap;
use ultar_driverpkg::{candidates, Candidate, DeviceDescriptor, DriverIndex};

/// Settings of the device manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// ABI of the running kernel, e.g. `ultar-kern-abi-1`; drivers for other kernels are ignored.
    pub kernel_abi: String,
    /// When every regular candidate failed, retry the quarantined ones once before giving up
    /// (keeps a machine usable when its only storage or display driver was quarantined by a
    /// transient fault). A driver that works again leaves the quarantine.
    pub last_resort_retry: bool,
}

impl Config {
    /// The default settings for a kernel ABI.
    pub fn new(kernel_abi: impl Into<String>) -> Self {
        Self {
            kernel_abi: kernel_abi.into(),
            last_resort_retry: true,
        }
    }
}

/// What the manager knows about one present device.
#[derive(Debug, Clone)]
pub(crate) struct Record {
    pub(crate) device: DeviceDescriptor,
    pub(crate) phase: Phase,
    pub(crate) active: Option<Candidate>,
    pub(crate) capabilities: Vec<Capability>,
}

/// The runtime core of `ultar-devd`.
///
/// Generic over the loader `L`, health check `H`, persistent store `S` and capability sink `K`.
pub struct DeviceManager<L, H, S, K> {
    pub(crate) loader: L,
    pub(crate) health: H,
    pub(crate) store: S,
    pub(crate) sink: K,
    pub(crate) index: DriverIndex,
    pub(crate) config: Config,
    pub(crate) devices: BTreeMap<String, Record>,
    pub(crate) state: StoredState,
    saved: StoredState,
    store_error: Option<StoreError>,
}

impl<L: DriverLoader, H: HealthCheck, S: ChoiceStore, K: CapabilitySink> DeviceManager<L, H, S, K> {
    /// Creates a manager and reads the remembered decisions from the store.
    ///
    /// If the store cannot be read the manager starts without memory (every device is
    /// searched afresh); see [`DeviceManager::store_error`].
    pub fn new(
        config: Config,
        index: DriverIndex,
        loader: L,
        health: H,
        mut store: S,
        sink: K,
    ) -> Self {
        let (state, store_error) = match store.load() {
            Ok(state) => (state, None),
            Err(error) => (StoredState::default(), Some(error)),
        };
        Self {
            loader,
            health,
            store,
            sink,
            index,
            config,
            devices: BTreeMap::new(),
            saved: state.clone(),
            state,
            store_error,
        }
    }

    /// The most recent store failure; `None` once the state was saved successfully.
    pub fn store_error(&self) -> Option<StoreError> {
        self.store_error
    }

    /// The decisions currently in effect (choices, previous drivers, quarantine).
    pub fn state(&self) -> &StoredState {
        &self.state
    }

    /// All quarantined drivers.
    pub fn quarantine(&self) -> &[QuarantineEntry] {
        &self.state.quarantine
    }

    /// The phase of the device at `address`, `None` when no such device is present.
    pub fn phase(&self, address: &str) -> Option<Phase> {
        self.devices.get(address).map(|record| record.phase)
    }

    /// The driver loader.
    pub fn loader(&self) -> &L {
        &self.loader
    }

    /// The health checker.
    pub fn health(&self) -> &H {
        &self.health
    }

    /// The persistent store.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// The persistent store, mutable (to collect the store's own events).
    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    /// The capability sink.
    pub fn sink(&self) -> &K {
        &self.sink
    }

    /// Installs a new depot index (e.g. after a driver update). Devices keep their drivers;
    /// call [`DeviceManager::rescan`] for devices that should be searched again.
    pub fn replace_index(&mut self, index: DriverIndex) {
        self.index = index;
    }

    /// Handles a device event: probes a new device or releases a removed one.
    pub fn handle_event(&mut self, event: DeviceEvent) -> Result<ProbeReport, DevdError> {
        match event {
            DeviceEvent::Added(device) => self.device_added(device),
            DeviceEvent::Removed(address) => {
                let mut report = ProbeReport::new(&address);
                self.remove(&address, &mut report)?;
                Ok(report)
            }
        }
    }

    fn device_added(&mut self, device: DeviceDescriptor) -> Result<ProbeReport, DevdError> {
        let address = device.address.clone();
        let mut report = ProbeReport::new(&address);
        if let Some(record) = self.devices.get_mut(&address) {
            if record.device.identity() == device.identity() {
                record.device = device;
                return match record.phase {
                    Phase::NoDriver | Phase::Failed => self.rescan(&address),
                    _ => {
                        report.active = record.active.as_ref().map(crate::types::DriverKey::of);
                        Ok(report)
                    }
                };
            }
            // Different hardware at the same address: the old device is gone.
            self.remove(&address, &mut report)?;
        }
        let record = Record {
            device,
            phase: Phase::Detected,
            active: None,
            capabilities: Vec::new(),
        };
        self.devices.insert(address.clone(), record);
        report.transitions.push(Phase::Detected);
        self.probe(&address, &mut report)?;
        self.persist(&mut report);
        Ok(report)
    }

    /// Unloads the active driver (if any) and forgets the device. Remembered choices stay.
    pub(crate) fn remove(
        &mut self,
        address: &str,
        report: &mut ProbeReport,
    ) -> Result<(), DevdError> {
        self.step(address, Signal::Unplugged, report)?;
        self.deactivate(address);
        self.devices.remove(address);
        Ok(())
    }

    /// Applies a signal to the device's state machine and records the new phase.
    pub(crate) fn step(
        &mut self,
        address: &str,
        signal: Signal,
        report: &mut ProbeReport,
    ) -> Result<Phase, DevdError> {
        let record = self
            .devices
            .get_mut(address)
            .ok_or_else(|| DevdError::UnknownDevice(address.to_string()))?;
        let next = transition(record.phase, signal)?;
        record.phase = next;
        report.transitions.push(next);
        Ok(next)
    }

    /// The drivers that fit a device on the running kernel, best first.
    pub(crate) fn candidates_for(&self, device: &DeviceDescriptor) -> Vec<Candidate> {
        candidates(device, &self.index, &self.config.kernel_abi)
    }

    /// Saves the state when it changed since the last successful save.
    pub(crate) fn persist(&mut self, report: &mut ProbeReport) {
        if self.state != self.saved {
            match self.store.save(&self.state) {
                Ok(()) => {
                    self.saved = self.state.clone();
                    self.store_error = None;
                }
                Err(error) => self.store_error = Some(error),
            }
        }
        report.store_error = self.store_error;
    }
}
