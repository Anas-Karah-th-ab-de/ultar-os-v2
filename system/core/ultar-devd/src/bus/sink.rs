//! Publishes capabilities and driver states.

use crate::traits::CapabilitySink;
use crate::types::{CapabilityChange, ChangeKind};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use ultar_bus::messages::devices::{
    Availability, Capabilities, Capability, DriverPhase, DriverState,
};
use ultar_bus::{BusError, Client, Text};

/// Message key of the note on a capability whose hardware has no working driver.
pub const NOTE_NO_DRIVER: &str = "devd.capability.no-driver";
/// Most messages a [`MemoryPublisher`] keeps per topic.
pub const MAX_RECORDED: usize = 1024;

/// Where the sink sends its messages.
pub trait Publisher: Send {
    /// Publishes the complete capability set (retained topic `devices.capabilities`).
    fn capabilities(&mut self, message: &Capabilities) -> Result<(), BusError>;
    /// Publishes one driver state change (topic `devices.driver_state`).
    fn driver_state(&mut self, message: &DriverState) -> Result<(), BusError>;
}

impl Publisher for Client {
    fn capabilities(&mut self, message: &Capabilities) -> Result<(), BusError> {
        self.publish(message)
    }

    fn driver_state(&mut self, message: &DriverState) -> Result<(), BusError> {
        self.publish(message)
    }
}

/// Everything a [`MemoryPublisher`] received.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Published {
    /// Capability sets in publishing order.
    pub capabilities: Vec<Capabilities>,
    /// Driver states in publishing order.
    pub states: Vec<DriverState>,
}

/// Keeps the messages in memory (bounded): for `ultar-devd --once` and for tests.
#[derive(Debug, Clone, Default)]
pub struct MemoryPublisher(Arc<Mutex<Published>>);

impl MemoryPublisher {
    /// A copy of everything received so far.
    pub fn published(&self) -> Published {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

fn push_bounded<T>(list: &mut Vec<T>, item: T) {
    if list.len() >= MAX_RECORDED {
        list.remove(0);
    }
    list.push(item);
}

impl Publisher for MemoryPublisher {
    fn capabilities(&mut self, message: &Capabilities) -> Result<(), BusError> {
        let mut published = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        push_bounded(&mut published.capabilities, message.clone());
        Ok(())
    }

    fn driver_state(&mut self, message: &DriverState) -> Result<(), BusError> {
        let mut published = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        push_bounded(&mut published.states, message.clone());
        Ok(())
    }
}

/// A present device without working driver, as the daemon reports it to the sink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnservedDevice {
    /// Address of the device.
    pub address: String,
    /// The capability the hardware would provide, if its class tells.
    pub hint: Option<&'static str>,
    /// Best fitting driver package, empty when none fits.
    pub driver: String,
    /// `Missing` (no package) or `Failed` (packages fit, none usable).
    pub phase: DriverPhase,
    /// Why, for display.
    pub reason: Option<Text>,
}

struct Inner {
    publisher: Box<dyn Publisher>,
    /// Capability names per device with an active driver.
    active: BTreeMap<String, Vec<String>>,
    /// Capability hints of devices without working driver.
    unserved: BTreeMap<String, Option<&'static str>>,
    /// Last published state per device (never `Removed`).
    states: BTreeMap<String, DriverState>,
    revision: u64,
    last: Option<Vec<Capability>>,
    held: bool,
    errors: u64,
}

impl Inner {
    fn set_state(&mut self, state: DriverState) {
        let removed = state.phase == DriverPhase::Removed;
        match self.states.get(&state.device) {
            Some(known) if known == &state => return,
            None if removed => return,
            _ => {}
        }
        if self.publisher.driver_state(&state).is_err() {
            self.errors += 1;
        }
        if removed {
            self.states.remove(&state.device);
        } else {
            self.states.insert(state.device.clone(), state);
        }
    }

    fn current(&self) -> Vec<Capability> {
        let mut set: BTreeMap<&str, Availability> = BTreeMap::new();
        for hint in self.unserved.values().flatten() {
            set.insert(hint, Availability::Unavailable);
        }
        for name in self.active.values().flatten() {
            set.insert(name, Availability::Available);
        }
        set.into_iter()
            .map(|(id, availability)| Capability {
                id: id.to_string(),
                availability,
                note: (availability == Availability::Unavailable)
                    .then(|| Text::key(NOTE_NO_DRIVER)),
            })
            .collect()
    }

    fn publish_capabilities(&mut self) {
        let capabilities = self.current();
        if self.last.as_ref() == Some(&capabilities) {
            return;
        }
        self.revision += 1;
        let message = Capabilities {
            revision: self.revision,
            capabilities,
        };
        match self.publisher.capabilities(&message) {
            Ok(()) => self.last = Some(message.capabilities),
            Err(_) => {
                // Not delivered: forget what was sent so the next change publishes again.
                self.errors += 1;
                self.last = None;
            }
        }
    }
}

/// The capability sink of the daemon. Clones share one state: the manager owns one, the
/// daemon keeps another to report devices without driver and to batch updates.
#[derive(Clone)]
pub struct BusSink {
    inner: Arc<Mutex<Inner>>,
}

impl BusSink {
    /// Creates a sink publishing through `publisher`.
    pub fn new(publisher: Box<dyn Publisher>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                publisher,
                active: BTreeMap::new(),
                unserved: BTreeMap::new(),
                states: BTreeMap::new(),
                revision: 0,
                last: None,
                held: false,
                errors: 0,
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Stops publishing capability sets until [`BusSink::release`]: one operation that
    /// touches many devices then produces one retained message instead of many.
    pub fn hold(&self) {
        self.lock().held = true;
    }

    /// Ends [`BusSink::hold`] and publishes the capability set when it differs from the
    /// last one published (always at the first call).
    pub fn release(&self) {
        let mut inner = self.lock();
        inner.held = false;
        inner.publish_capabilities();
    }

    /// Replaces the list of present devices without working driver: publishes their
    /// `Missing`/`Failed` states (only changes) and `Removed` for devices no longer listed.
    pub fn sync_unserved(&self, devices: Vec<UnservedDevice>) {
        let mut inner = self.lock();
        let gone: Vec<String> = inner
            .states
            .iter()
            .filter(|(address, state)| {
                state.phase != DriverPhase::Active
                    && !devices.iter().any(|d| &d.address == *address)
            })
            .map(|(address, _)| address.clone())
            .collect();
        for address in gone {
            inner.set_state(DriverState {
                device: address,
                driver: String::new(),
                phase: DriverPhase::Removed,
                reason: None,
            });
        }
        inner.unserved = devices
            .iter()
            .map(|d| (d.address.clone(), d.hint))
            .collect();
        for device in devices {
            inner.set_state(DriverState {
                device: device.address,
                driver: device.driver,
                phase: device.phase,
                reason: device.reason,
            });
        }
        if !inner.held {
            inner.publish_capabilities();
        }
    }

    /// The capability set as it would be published now.
    pub fn snapshot(&self) -> Capabilities {
        let inner = self.lock();
        Capabilities {
            revision: inner.revision,
            capabilities: inner.current(),
        }
    }

    /// Number of messages the publisher refused so far.
    pub fn publish_errors(&self) -> u64 {
        self.lock().errors
    }
}

impl CapabilitySink for BusSink {
    fn emit(&mut self, change: CapabilityChange) {
        let mut inner = self.lock();
        let phase = match change.kind {
            ChangeKind::Gained => {
                let names = change.capabilities.iter().map(|c| c.name.clone()).collect();
                inner.active.insert(change.device.clone(), names);
                inner.unserved.remove(&change.device);
                DriverPhase::Active
            }
            ChangeKind::Lost => {
                inner.active.remove(&change.device);
                DriverPhase::Removed
            }
        };
        inner.set_state(DriverState {
            device: change.device,
            driver: change.driver.id,
            phase,
            reason: None,
        });
        if !inner.held {
            inner.publish_capabilities();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Capability as Provided, DriverKey};
    use ultar_driverpkg::{DriverClass, Version};

    fn change(device: &str, kind: ChangeKind, capability: &str) -> CapabilityChange {
        CapabilityChange {
            device: device.into(),
            driver: DriverKey::new("drv", Version::new(1, 0, 0)),
            class: DriverClass::Net,
            kind,
            capabilities: vec![Provided::new(capability, "present")],
        }
    }

    fn unserved(address: &str, hint: Option<&'static str>, phase: DriverPhase) -> UnservedDevice {
        UnservedDevice {
            address: address.into(),
            hint,
            driver: String::new(),
            phase,
            reason: Some(Text::key("devd.reason.no-package")),
        }
    }

    fn ids(message: &Capabilities) -> Vec<(String, Availability)> {
        message
            .capabilities
            .iter()
            .map(|c| (c.id.clone(), c.availability))
            .collect()
    }

    #[test]
    fn publishes_complete_sets_with_rising_revision() {
        let memory = MemoryPublisher::default();
        let mut sink = BusSink::new(Box::new(memory.clone()));
        sink.emit(change("pci:1", ChangeKind::Gained, "net"));
        sink.emit(change("pci:2", ChangeKind::Gained, "net"));
        sink.emit(change("pci:3", ChangeKind::Gained, "storage"));
        sink.emit(change("pci:1", ChangeKind::Lost, "net"));
        sink.emit(change("pci:2", ChangeKind::Lost, "net"));
        let published = memory.published();
        // The second network card and the loss of the first change nothing in the set.
        let revisions: Vec<u64> = published.capabilities.iter().map(|c| c.revision).collect();
        assert_eq!(revisions, [1, 2, 3]);
        assert_eq!(ids(&published.capabilities[1]).len(), 2);
        assert_eq!(
            ids(&published.capabilities[2]),
            [("storage".to_string(), Availability::Available)]
        );
        let phases: Vec<DriverPhase> = published.states.iter().map(|s| s.phase).collect();
        assert_eq!(
            phases,
            [
                DriverPhase::Active,
                DriverPhase::Active,
                DriverPhase::Active,
                DriverPhase::Removed,
                DriverPhase::Removed
            ]
        );
        assert_eq!(sink.snapshot().revision, 3);
        assert_eq!(sink.publish_errors(), 0);
    }

    #[test]
    fn hold_batches_and_unserved_devices_are_reported_once() {
        let memory = MemoryPublisher::default();
        let mut sink = BusSink::new(Box::new(memory.clone()));
        sink.hold();
        sink.emit(change("pci:1", ChangeKind::Gained, "net"));
        sink.emit(change("pci:2", ChangeKind::Gained, "storage"));
        let rows = vec![
            unserved("pci:3", Some("wifi"), DriverPhase::Failed),
            unserved("pci:4", Some("net"), DriverPhase::Missing),
            unserved("pci:5", None, DriverPhase::Missing),
        ];
        sink.sync_unserved(rows.clone());
        assert!(memory.published().capabilities.is_empty());
        sink.release();
        sink.release();
        sink.sync_unserved(rows);
        let published = memory.published();
        assert_eq!(published.capabilities.len(), 1);
        assert_eq!(
            ids(&published.capabilities[0]),
            [
                ("net".to_string(), Availability::Available),
                ("storage".to_string(), Availability::Available),
                ("wifi".to_string(), Availability::Unavailable),
            ]
        );
        let wifi = published.capabilities[0].get("wifi").unwrap();
        assert_eq!(wifi.note, Some(Text::key(NOTE_NO_DRIVER)));
        assert_eq!(
            published.states.len(),
            5,
            "repeated sync publishes nothing new"
        );
        // The wifi card gets a driver, another device disappears.
        sink.emit(change("pci:3", ChangeKind::Gained, "wifi"));
        sink.sync_unserved(vec![unserved("pci:4", Some("net"), DriverPhase::Missing)]);
        let published = memory.published();
        let last = published.states.last().unwrap();
        assert_eq!(
            (last.device.as_str(), last.phase),
            ("pci:5", DriverPhase::Removed)
        );
        assert!(published.capabilities.last().unwrap().has("wifi"));
    }

    struct Refusing;

    impl Publisher for Refusing {
        fn capabilities(&mut self, _: &Capabilities) -> Result<(), BusError> {
            Err(BusError::NotConnected)
        }
        fn driver_state(&mut self, _: &DriverState) -> Result<(), BusError> {
            Err(BusError::NotConnected)
        }
    }

    #[test]
    fn publish_failures_are_counted_and_retried() {
        let mut sink = BusSink::new(Box::new(Refusing));
        sink.emit(change("pci:1", ChangeKind::Gained, "net"));
        assert_eq!(sink.publish_errors(), 2);
        // Nothing changed, but the last set never arrived: it is sent again.
        sink.release();
        assert_eq!(sink.publish_errors(), 3);
        assert_eq!(sink.snapshot().capabilities.len(), 1);
    }

    #[test]
    fn memory_publisher_is_bounded() {
        let mut memory = MemoryPublisher::default();
        for revision in 0..MAX_RECORDED as u64 + 5 {
            memory
                .capabilities(&Capabilities {
                    revision,
                    capabilities: Vec::new(),
                })
                .unwrap();
        }
        let published = memory.published();
        assert_eq!(published.capabilities.len(), MAX_RECORDED);
        assert_eq!(published.capabilities[0].revision, 5);
    }
}
