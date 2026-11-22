//! The running service: enumerate → resolve → manage → publish, then serve the bus
//! service `devices`.
//!
//! Start-up never fails because of a missing depot, missing keys, unreadable hardware or a
//! damaged state file: the daemon then runs with what it has and says so in the notices
//! of `devices.list`. The only hard start error is an unknown kernel (no driver can be
//! matched without its ABI name).
//!
//! **Hot-plug:** the Ultar kernel has no device event interface. Devices are read at
//! start and again on `devices.rescan`.
//!
//! **Shutdown:** `std` offers no signal handling and the crate forbids `unsafe`, so the
//! clean way to stop is the bus method `devices.shutdown`. A plain SIGTERM ends the
//! process at once; that is safe at any moment because every state change is already on
//! disk through an atomic replace before the reply is sent.

use crate::builtin::Builtin;
use crate::bus::convert::{device_info, reason, refusal, refuse, unserved_phase};
use crate::bus::{BusSink, Publisher, UnservedDevice};
use crate::caps::hardware_hint;
use crate::depot;
use crate::fsm::Phase;
use crate::manager::{Config, DeviceManager};
use crate::ops::QuarantineFilter;
use crate::store::file::{FileStore, StoreEvent};
use crate::types::{DeviceEvent, DriverKey};
use std::collections::BTreeSet;
use std::path::PathBuf;
use ultar_bus::messages::device_service::{
    ClearQuarantine, ClearQuarantineReply, DeviceInfo, ListReply, OperationReply, Outcome, Refusal,
    RefusalKind, RescanReply,
};
use ultar_bus::{Text, TextArg};
use ultar_driverpkg::Version;
use ultar_hal_system::{kernel_abi, ControlInterface, HardwareError, HardwareReader};

/// Where the daemon finds its inputs and keeps its state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    /// Root of the system description (`/`; a prepared tree in tests).
    pub root: PathBuf,
    /// Driver depot of the installed system (`*.utr`, `index.toml`, `index.toml.sig`).
    pub depot: PathBuf,
    /// Directory with the trusted public keys (`*.keys`).
    pub keys: PathBuf,
    /// Directory for the state file.
    pub state: PathBuf,
    /// Kernel ABI name; detected from the system when `None`.
    pub kernel_abi: Option<String>,
}

/// Why the daemon could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// The kernel ABI is neither given nor detectable.
    UnknownKernel,
    /// The system description exists but cannot be read.
    Hardware(HardwareError),
}

impl StartError {
    /// Stable message key.
    pub fn key(&self) -> &'static str {
        match self {
            Self::UnknownKernel => "devd.start.unknown-kernel",
            Self::Hardware(_) => "devd.start.hardware",
        }
    }
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownKernel => f.write_str(self.key()),
            Self::Hardware(error) => write!(f, "{}: {error}", self.key()),
        }
    }
}

impl std::error::Error for StartError {}

impl From<HardwareError> for StartError {
    fn from(error: HardwareError) -> Self {
        Self::Hardware(error)
    }
}

type Manager = DeviceManager<Builtin, Builtin, FileStore, BusSink>;

/// What changed during a scan of the hardware.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScanResult {
    /// Addresses that appeared.
    pub added: Vec<String>,
    /// Addresses that disappeared.
    pub removed: Vec<String>,
}

/// The device daemon without its bus connection (see [`Daemon::serve`]).
pub struct Daemon {
    pub(crate) manager: Manager,
    pub(crate) sink: BusSink,
    pub(crate) builtin: Builtin,
    reader: HardwareReader,
    config: DaemonConfig,
    depot_notices: Vec<Text>,
    hardware_notices: Vec<Text>,
    /// Set when the state file was damaged at start; stays until the daemon restarts.
    recovery_notice: Option<Text>,
    /// Set once a bus message could not be published.
    publish_failed: bool,
}

impl Daemon {
    /// Starts the daemon: reads keys, depot, state and hardware, resolves every device and
    /// publishes the result through `publisher`.
    pub fn start(config: DaemonConfig, publisher: Box<dyn Publisher>) -> Result<Self, StartError> {
        let reader = HardwareReader::at(&config.root);
        let abi = match &config.kernel_abi {
            Some(abi) => abi.clone(),
            None => kernel_abi(&config.root)?.ok_or(StartError::UnknownKernel)?,
        };
        let depot = depot::load(&config.depot, &config.keys);
        let builtin = Builtin::new(reader.clone(), &config.depot, depot.trusted)?;
        let sink = BusSink::new(publisher);
        let manager = DeviceManager::new(
            Config::new(abi),
            depot.index,
            builtin.clone(),
            builtin.clone(),
            FileStore::new(&config.state),
            sink.clone(),
        );
        let mut daemon = Self {
            manager,
            sink,
            builtin,
            reader,
            config,
            depot_notices: depot.notices,
            hardware_notices: Vec::new(),
            recovery_notice: None,
            publish_failed: false,
        };
        // An unreadable inventory is a notice, not a start failure.
        let _ = daemon.scan();
        Ok(daemon)
    }

    /// Reads the hardware again and feeds the differences to the manager.
    pub fn scan(&mut self) -> Result<ScanResult, Refusal> {
        let inventory = match self.reader.inventory() {
            Ok(inventory) => inventory,
            Err(error) => {
                self.hardware_notices = vec![Text::key("devd.notice.hardware-unreadable")
                    .with(TextArg::Key(error.key().to_string()))];
                self.publish();
                return Err(refuse(RefusalKind::SystemUnreadable, error.key()));
            }
        };
        self.hardware_notices = inventory
            .notes
            .iter()
            .map(|note| Text::key(note.key()))
            .collect();
        let known: BTreeSet<String> = self
            .manager
            .devices()
            .into_iter()
            .map(|view| view.device.address)
            .collect();
        let present: BTreeSet<&str> = inventory
            .devices
            .iter()
            .map(|d| d.address.as_str())
            .collect();
        let mut result = ScanResult::default();
        self.sink.hold();
        for address in known.iter().filter(|a| !present.contains(a.as_str())) {
            if self
                .manager
                .handle_event(DeviceEvent::Removed(address.clone()))
                .is_ok()
            {
                self.builtin.forget(address);
                result.removed.push(address.clone());
            }
        }
        for device in inventory.devices {
            let address = device.address.clone();
            let outcome = self.manager.handle_event(DeviceEvent::Added(device));
            if outcome.is_ok() && !known.contains(&address) {
                result.added.push(address);
            }
        }
        self.publish();
        Ok(result)
    }

    /// Publishes what changed and collects the store's own events.
    fn publish(&mut self) {
        let unserved = self
            .manager
            .devices()
            .iter()
            .filter(|view| view.phase != Phase::Active)
            .map(|view| UnservedDevice {
                address: view.device.address.clone(),
                hint: hardware_hint(&view.device.primary),
                driver: view
                    .alternatives
                    .first()
                    .map(|a| a.driver.id.clone())
                    .unwrap_or_default(),
                phase: unserved_phase(view.phase),
                reason: reason(view, &self.builtin),
            })
            .collect();
        self.sink.sync_unserved(unserved);
        self.sink.release();
        for event in self.manager.store_mut().take_events() {
            let StoreEvent::Recovered { problem, .. } = event;
            let notice = Text::key("devd.notice.state-recovered");
            self.recovery_notice = Some(notice.with(TextArg::Key(problem.key().to_string())));
        }
        self.publish_failed = self.sink.publish_errors() > 0;
    }

    fn row(&self, address: &str) -> Option<DeviceInfo> {
        self.manager
            .devices()
            .iter()
            .find(|view| view.device.address == address)
            .map(|view| device_info(view, &self.builtin))
    }

    fn rows(&self) -> Vec<DeviceInfo> {
        self.manager
            .devices()
            .iter()
            .map(|view| device_info(view, &self.builtin))
            .collect()
    }

    /// Notices about the daemon's own state file and bus messages, computed live.
    fn own_notices(&self) -> Vec<Text> {
        let mut notices: Vec<Text> = self.recovery_notice.iter().cloned().collect();
        if let Some(error) = self.manager.store_error() {
            let notice = Text::key("devd.notice.state-unavailable");
            notices.push(notice.with(TextArg::Key(error.to_string())));
        }
        if self.publish_failed {
            notices.push(Text::key("devd.notice.publish-failed"));
        }
        notices
    }

    /// `devices.list`.
    pub fn list(&self) -> ListReply {
        let mut notices = self.depot_notices.clone();
        notices.extend(self.hardware_notices.iter().cloned());
        notices.extend(self.own_notices());
        ListReply {
            devices: self.rows(),
            notices,
            // The loader never changes kernel bindings (see `crate::builtin`).
            kernel_switch_supported: false,
        }
    }

    /// The decisions currently in effect (choices, roll-back targets, quarantine).
    pub fn state(&self) -> &crate::StoredState {
        self.manager.state()
    }

    /// Number of times the state file was written since start.
    pub fn state_saves(&self) -> u64 {
        self.manager.store().saves()
    }

    /// The capability set as last computed for `devices.capabilities`.
    pub fn capabilities(&self) -> ultar_bus::messages::devices::Capabilities {
        self.sink.snapshot()
    }

    /// The interface the running kernel offers for changing driver bindings (reported
    /// only; this daemon uses none).
    pub fn kernel_control(&self) -> ControlInterface {
        self.builtin.control_interface()
    }

    fn finish(
        &mut self,
        address: &str,
        result: Result<crate::types::ProbeReport, crate::DevdError>,
    ) -> OperationReply {
        match result {
            Ok(report) => {
                self.publish();
                let outcome = match self.row(address) {
                    Some(row) => Outcome::Done(Box::new(row)),
                    // The device vanished during the operation.
                    None => Outcome::Refused(refusal(&crate::DevdError::UnknownDevice(
                        address.to_string(),
                    ))),
                };
                OperationReply {
                    outcome,
                    restored: report.restored,
                }
            }
            Err(error) => OperationReply {
                outcome: Outcome::Refused(refusal(&error)),
                restored: false,
            },
        }
    }

    /// `devices.switch`.
    pub fn switch(&mut self, address: &str, driver: &str, version: &str) -> OperationReply {
        let Ok(version) = Version::parse(version) else {
            return OperationReply {
                outcome: Outcome::Refused(refuse(
                    RefusalKind::BadRequest,
                    "devd.error.bad-request",
                )),
                restored: false,
            };
        };
        let key = DriverKey::new(driver, version);
        let result = self.manager.switch_driver(address, &key);
        self.finish(address, result)
    }

    /// `devices.rollback`.
    pub fn rollback(&mut self, address: &str) -> OperationReply {
        let result = self.manager.roll_back(address);
        self.finish(address, result)
    }

    /// `devices.clear_quarantine`.
    pub fn clear_quarantine(&mut self, request: ClearQuarantine) -> ClearQuarantineReply {
        let bad = || refuse(RefusalKind::BadRequest, "devd.error.bad-request");
        let filter = match (request.address, request.driver) {
            (None, None) => Ok(QuarantineFilter::All),
            (Some(address), None) => Ok(QuarantineFilter::Device(address)),
            (Some(device), Some((id, version))) => Version::parse(&version)
                .map(|version| QuarantineFilter::Driver {
                    device,
                    driver: DriverKey::new(id, version),
                })
                .map_err(|_| bad()),
            (None, Some(_)) => Err(bad()),
        };
        let released = filter.and_then(|filter| {
            self.manager
                .clear_quarantine(&filter)
                .map(|count| u32::try_from(count).unwrap_or(u32::MAX))
                .map_err(|error| refusal(&error))
        });
        if released.is_ok() {
            self.publish();
        }
        ClearQuarantineReply { released }
    }

    /// `devices.rescan`: one driverless device, or (without address) keys, depot, kernel
    /// bindings and hardware.
    pub fn rescan(&mut self, address: Option<&str>) -> RescanReply {
        let mut scan = ScanResult::default();
        let result = match address {
            Some(address) => self
                .manager
                .rescan(address)
                .map(|_| self.publish())
                .map_err(|error| refusal(&error)),
            None => self.reload().map(|result| scan = result),
        };
        RescanReply {
            devices: result.map(|()| self.rows()),
            added: scan.added,
            removed: scan.removed,
        }
    }

    fn reload(&mut self) -> Result<ScanResult, Refusal> {
        let depot = depot::load(&self.config.depot, &self.config.keys);
        self.depot_notices = depot.notices;
        self.builtin.set_trusted(depot.trusted);
        self.manager.replace_index(depot.index);
        self.builtin
            .refresh()
            .map_err(|error| refuse(RefusalKind::SystemUnreadable, error.key()))?;
        self.scan()
    }
}
