//! Mock loader, health check and sink plus a small test rig.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use ultar_devd::{
    Capability, CapabilityChange, CapabilitySink, ChangeKind, ChoiceStore, Config, DeviceEvent,
    DeviceManager, DriverKey, DriverLoader, HealthCheck, HealthVerdict, LoadError, MemoryStore,
    ProbeReport, StoreError, StoredState,
};
use ultar_driverpkg::index::IndexEntry;
use ultar_driverpkg::package::{pack, Limits};
use ultar_driverpkg::payload::build_payload_from;
use ultar_driverpkg::{
    Candidate, DeviceDescriptor, DriverIndex, HardwareId, PrivateKey, TrustedKeys, Version,
};

pub const ABI: &str = "ultar-kern-abi-1";
pub const NIC: &str = "pci:00:1f.6";

/// Scripted behaviour and a record of everything the manager did.
#[derive(Default)]
pub struct World {
    /// Load failures by `driver` or `driver@address`.
    pub load_errors: HashMap<String, LoadError>,
    /// Health verdicts by `driver` or `driver@address`; default is healthy.
    pub verdicts: HashMap<String, HealthVerdict>,
    /// Drivers the loader says it cannot switch to.
    pub no_switch: Vec<String>,
    /// `load|check|unload <driver> <address>` in call order.
    pub log: Vec<String>,
    /// Every emitted capability change.
    pub events: Vec<CapabilityChange>,
}

pub type Shared = Rc<RefCell<World>>;

fn scripted<T: Clone>(map: &HashMap<String, T>, driver: &str, address: &str) -> Option<T> {
    map.get(&format!("{driver}@{address}"))
        .or_else(|| map.get(driver))
        .cloned()
}

pub struct MockLoader(pub Shared);

impl DriverLoader for MockLoader {
    fn load(&mut self, device: &DeviceDescriptor, driver: &Candidate) -> Result<(), LoadError> {
        let mut world = self.0.borrow_mut();
        world
            .log
            .push(format!("load {} {}", driver.id, device.address));
        match scripted(&world.load_errors, &driver.id, &device.address) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn unload(&mut self, device: &DeviceDescriptor, driver: &Candidate) {
        self.0
            .borrow_mut()
            .log
            .push(format!("unload {} {}", driver.id, device.address));
    }

    fn can_switch(&mut self, _device: &DeviceDescriptor, target: &Candidate) -> bool {
        !self.0.borrow().no_switch.contains(&target.id)
    }
}

pub struct MockHealth(pub Shared);

impl HealthCheck for MockHealth {
    fn check(&mut self, device: &DeviceDescriptor, driver: &Candidate) -> HealthVerdict {
        let mut world = self.0.borrow_mut();
        world
            .log
            .push(format!("check {} {}", driver.id, device.address));
        scripted(&world.verdicts, &driver.id, &device.address).unwrap_or_else(|| {
            HealthVerdict::Healthy(vec![Capability::new("driver", driver.id.clone())])
        })
    }
}

pub struct MockSink(pub Shared);

impl CapabilitySink for MockSink {
    fn emit(&mut self, change: CapabilityChange) {
        self.0.borrow_mut().events.push(change);
    }
}

/// A store whose failures can be switched on and off.
#[derive(Default)]
pub struct FlakyStore {
    pub inner: MemoryStore,
    pub fail_load: Option<StoreError>,
    pub fail_save: Rc<RefCell<Option<StoreError>>>,
}

impl ChoiceStore for FlakyStore {
    fn load(&mut self) -> Result<StoredState, StoreError> {
        match self.fail_load {
            Some(error) => Err(error),
            None => self.inner.load(),
        }
    }

    fn save(&mut self, state: &StoredState) -> Result<(), StoreError> {
        match *self.fail_save.borrow() {
            Some(error) => Err(error),
            None => self.inner.save(state),
        }
    }
}

pub type Manager<S = MemoryStore> = DeviceManager<MockLoader, MockHealth, S, MockSink>;

/// A manager with mocks ("one boot").
pub struct Rig<S: ChoiceStore = MemoryStore> {
    pub world: Shared,
    pub manager: Manager<S>,
}

pub fn boot_with<S: ChoiceStore>(index: &DriverIndex, store: S, config: Config) -> Rig<S> {
    let world = Shared::default();
    let (loader, health, sink) = (
        MockLoader(world.clone()),
        MockHealth(world.clone()),
        MockSink(world.clone()),
    );
    Rig {
        manager: DeviceManager::new(config, index.clone(), loader, health, store, sink),
        world,
    }
}

/// Boots with the given remembered state (as if read from disk).
pub fn boot(index: &DriverIndex, state: StoredState) -> Rig {
    boot_with(index, MemoryStore::with_state(state), Config::new(ABI))
}

impl<S: ChoiceStore> Rig<S> {
    pub fn add(&mut self, device: DeviceDescriptor) -> ProbeReport {
        self.manager
            .handle_event(DeviceEvent::Added(device))
            .unwrap()
    }

    pub fn fail_load(&self, driver: &str, error: LoadError) {
        self.world
            .borrow_mut()
            .load_errors
            .insert(driver.to_string(), error);
    }

    pub fn verdict(&self, driver: &str, verdict: HealthVerdict) {
        self.world
            .borrow_mut()
            .verdicts
            .insert(driver.to_string(), verdict);
    }

    pub fn heal(&self, driver: &str) {
        let mut world = self.world.borrow_mut();
        world.load_errors.remove(driver);
        world.verdicts.remove(driver);
    }

    pub fn log(&self) -> Vec<String> {
        self.world.borrow().log.clone()
    }

    pub fn take_log(&self) -> Vec<String> {
        std::mem::take(&mut self.world.borrow_mut().log)
    }

    /// Emitted changes as `+driver` (gained) / `-driver` (lost).
    pub fn events(&self) -> Vec<String> {
        let world = self.world.borrow();
        let sign = |kind| if kind == ChangeKind::Gained { '+' } else { '-' };
        world
            .events
            .iter()
            .map(|e| format!("{}{}", sign(e.kind), e.driver.id))
            .collect()
    }
}

impl Rig {
    /// The state a following boot would read: what the store holds, through its text form.
    pub fn stored(&self) -> StoredState {
        let text = self.manager.store().state().to_toml().unwrap();
        StoredState::from_toml(&text).unwrap()
    }
}

pub fn key(id: &str) -> DriverKey {
    DriverKey::new(id, Version::new(1, 0, 0))
}

pub fn nic() -> DeviceDescriptor {
    nic_at(NIC)
}

pub fn nic_at(address: &str) -> DeviceDescriptor {
    DeviceDescriptor::new(
        address,
        HardwareId::Pci {
            vendor: 0x8086,
            device: 0x0d4c,
            class: 0x020000,
            subsys: None,
        },
    )
}

pub fn manifest(id: &str, version: &str, rules: &str) -> String {
    format!(
        "id = \"{id}\"\nversion = \"{version}\"\nclass = \"net\"\nkernel_abi = [\"{ABI}\"]\norigin = \"workshop\"\n\
         name_key = \"driver.{id}.name\"\n[match]\n{rules}\n[content]\nmodule = \"drivers/{id}.uko\"\n"
    )
}

pub fn index_of(manifests: &[String]) -> DriverIndex {
    let key = PrivateKey::generate();
    let trusted = TrustedKeys::from(key.public());
    let limits = Limits::default();
    let entries = manifests
        .iter()
        .enumerate()
        .map(|(n, text)| {
            let payload =
                build_payload_from(&[(format!("drivers/{}.uko", id_of(text)), vec![1])]).unwrap();
            let bytes = pack(text.as_bytes(), &payload, &key, &limits).unwrap();
            IndexEntry::from_package(&format!("pkg{n}.utr"), &bytes, &trusted, &limits).unwrap()
        })
        .collect();
    DriverIndex::new(entries).unwrap()
}

fn id_of(manifest: &str) -> String {
    manifest.split('"').nth(1).unwrap().to_string()
}

/// Three drivers for the test NIC, best first: `alpha` (exact id), `beta` (vendor), `gamma` (class).
pub fn three_drivers() -> DriverIndex {
    index_of(&[
        manifest("alpha", "1.0.0", "pci = [\"8086:0d4c\"]"),
        manifest("beta", "1.0.0", "pci = [\"8086:*\"]"),
        manifest("gamma", "1.0.0", "pci_class = [\"0200\"]"),
    ])
}
