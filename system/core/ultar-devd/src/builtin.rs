//! Stage A of plan §5.5: drivers are built into the kernel, a package only *activates* one.
//!
//! A stage-A package carries an activation record `builtin/<id>.toml`:
//!
//! ```toml
//! kernel_driver = "gen-intel-eth"
//! ```
//!
//! "Loading" such a package means: verify the package signature against the trusted
//! keys, read the record, and confirm through the HAL system source that this kernel
//! driver operates the device. Nothing is written to the kernel, because the Ultar kernel
//! (0.4) has no interface for it:
//!
//! * `driver.rs::bind_all` binds once at boot, first match in the order of `drivers::ALL`;
//! * `/proc/ultar/` holds `verbund`, `firewall`, `sicherheit`, `geraet`, `anzeige`,
//!   `protokoll` – no driver or device node, and only `verbund`/`firewall` accept writes;
//! * no system call binds, unbinds, enables or disables a driver.
//!
//! Consequences implemented here, without pretending:
//!
//! * a package whose kernel driver did not bind the device cannot be made to work →
//!   [`LoadFailure::Unsupported`] (no quarantine: the package is not at fault);
//! * [`DriverLoader::unload`] changes nothing in the kernel (there is nothing to call);
//! * [`DriverLoader::can_switch`] is `true` only when the target package activates the
//!   kernel driver that already operates the device (another version or a machine-tuned
//!   variant of the same driver). Every other switch is refused as unsupported.
//!
//! Packages without activation record (loadable `.uko` modules, stage B) are answered
//! with [`LoadFailure::Unsupported`] as well: this loader cannot load modules.

use crate::caps::class_capability;
use crate::traits::{DriverLoader, HealthCheck, HealthVerdict, LoadError};
use crate::types::{Capability, DriverKey, HealthFailure, LoadFailure};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use ultar_driverpkg::package::{self, Limits};
use ultar_driverpkg::{Candidate, DeviceDescriptor, DriverClass, HardwareId, TrustedKeys};
use ultar_hal_system::hardware::device_info::KernelDeviceInfo;
use ultar_hal_system::{BindState, ControlInterface, HardwareError, HardwareReader, KernelDrivers};

/// Most load details remembered (one per device and driver).
pub const MAX_DETAILS: usize = 4096;

/// The exact reason of the last load attempt of a driver on a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadDetail {
    /// The package file is not in the depot directory.
    PackageMissing,
    /// Signature or structure verification failed; the code of the package error.
    PackageRejected(&'static str),
    /// The file holds another driver or version than the index says.
    WrongPackage,
    /// The package has no activation record: it needs loadable-module support (stage B).
    NoActivationRecord,
    /// The activation record is not valid.
    BadActivationRecord,
    /// The kernel's probe of this driver failed on the device (kernel error text).
    ProbeFailed(String),
    /// The kernel did not bind the package's driver to the device.
    NotBound {
        /// The kernel driver the package activates.
        kernel_driver: String,
        /// The kernel driver that operates the device instead, if any.
        bound_to: Option<String>,
    },
    /// The device is not in the kernel's description any more.
    DeviceGone,
    /// The driver was confirmed.
    Loaded {
        /// The kernel driver operating the device.
        kernel_driver: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivationRecord {
    kernel_driver: String,
}

fn valid_driver_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

struct Inner {
    reader: HardwareReader,
    depot: PathBuf,
    trusted: TrustedKeys,
    limits: Limits,
    drivers: KernelDrivers,
    info: Option<KernelDeviceInfo>,
    details: BTreeMap<(String, DriverKey), LoadDetail>,
}

/// Loader and health check for built-in kernel drivers. Clones share one state, so the
/// same value serves as [`DriverLoader`] and as [`HealthCheck`] of a manager.
#[derive(Clone)]
pub struct Builtin {
    inner: Arc<Mutex<Inner>>,
}

impl Builtin {
    /// Creates the loader and reads the kernel's drivers and bindings once.
    pub fn new(
        reader: HardwareReader,
        depot: &Path,
        trusted: TrustedKeys,
    ) -> Result<Self, HardwareError> {
        let drivers = KernelDrivers::read(&reader)?;
        let info = reader.device_info()?;
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                reader,
                depot: depot.to_path_buf(),
                trusted,
                limits: Limits::default(),
                drivers,
                info,
                details: BTreeMap::new(),
            })),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Reads the kernel's drivers and bindings again (after a rescan request).
    pub fn refresh(&self) -> Result<(), HardwareError> {
        let mut inner = self.lock();
        let drivers = KernelDrivers::read(&inner.reader)?;
        let info = inner.reader.device_info()?;
        inner.drivers = drivers;
        inner.info = info;
        Ok(())
    }

    /// Replaces the trusted keys (after the key directory was read again).
    pub fn set_trusted(&self, trusted: TrustedKeys) {
        self.lock().trusted = trusted;
    }

    /// The kernel's bind state of the device at `address`.
    pub fn binding(&self, address: &str) -> BindState {
        self.lock().drivers.binding(address)
    }

    /// The interface the kernel offers for changing bindings. The loader uses none: it
    /// reports what exists so the panel can say why switching is refused.
    pub fn control_interface(&self) -> ControlInterface {
        self.lock().drivers.control
    }

    /// Why the last load of `driver` on the device at `address` ended as it did.
    pub fn detail(&self, address: &str, driver: &DriverKey) -> Option<LoadDetail> {
        self.lock()
            .details
            .get(&(address.to_string(), driver.clone()))
            .cloned()
    }

    /// Forgets the load details of a device that is gone.
    pub fn forget(&self, address: &str) {
        self.lock()
            .details
            .retain(|(device, _), _| device != address);
    }
}

impl Inner {
    fn read_package(&self, file: &str) -> Result<Vec<u8>, LoadDetail> {
        if file.contains(['/', '\\']) || file.starts_with('.') {
            return Err(LoadDetail::PackageRejected("driverpkg.index.bad-file-name"));
        }
        let limit = self
            .limits
            .max_manifest
            .saturating_add(self.limits.max_payload)
            .saturating_add(self.limits.max_signature)
            .saturating_add(64 * 1024);
        let handle =
            std::fs::File::open(self.depot.join(file)).map_err(|_| LoadDetail::PackageMissing)?;
        let mut bytes = Vec::new();
        handle
            .take(limit.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| LoadDetail::PackageMissing)?;
        if bytes.len() as u64 > limit {
            return Err(LoadDetail::PackageRejected("driverpkg.package.too-large"));
        }
        Ok(bytes)
    }

    /// Verifies the candidate's package and returns the kernel driver it activates.
    fn kernel_driver(&self, candidate: &Candidate) -> Result<String, LoadDetail> {
        use ultar_driverpkg::Coded;
        let bytes = self.read_package(&candidate.file)?;
        let verified = package::verify(&bytes, &self.trusted, &self.limits)
            .map_err(|error| LoadDetail::PackageRejected(error.code()))?;
        if verified.manifest.id != candidate.id || verified.manifest.version != candidate.version {
            return Err(LoadDetail::WrongPackage);
        }
        let record = verified
            .read_file(&format!("builtin/{}.toml", candidate.id))
            .map_err(|error| LoadDetail::PackageRejected(error.code()))?
            .ok_or(LoadDetail::NoActivationRecord)?;
        let record: ActivationRecord = std::str::from_utf8(&record)
            .ok()
            .and_then(|text| toml::from_str(text).ok())
            .ok_or(LoadDetail::BadActivationRecord)?;
        if !valid_driver_name(&record.kernel_driver) {
            return Err(LoadDetail::BadActivationRecord);
        }
        Ok(record.kernel_driver)
    }

    /// The full stage-A "load": package checks, then the kernel's bind state.
    fn confirm(&self, device: &DeviceDescriptor, candidate: &Candidate) -> LoadDetail {
        let kernel_driver = match self.kernel_driver(candidate) {
            Ok(name) => name,
            Err(detail) => return detail,
        };
        match self.drivers.binding(&device.address) {
            BindState::Bound { driver } if driver == kernel_driver => {
                LoadDetail::Loaded { kernel_driver }
            }
            BindState::Bound { driver } => LoadDetail::NotBound {
                kernel_driver,
                bound_to: Some(driver),
            },
            BindState::Failed {
                driver: Some(driver),
                error,
            } if driver == kernel_driver => LoadDetail::ProbeFailed(error),
            BindState::Failed { .. } | BindState::Unbound => LoadDetail::NotBound {
                kernel_driver,
                bound_to: None,
            },
            BindState::Unknown => LoadDetail::DeviceGone,
        }
    }

    fn remember(&mut self, address: &str, driver: DriverKey, detail: LoadDetail) {
        if self.details.len() >= MAX_DETAILS {
            self.details.pop_first();
        }
        self.details.insert((address.to_string(), driver), detail);
    }

    /// The class-specific probe result the kernel exposes, if any: for a display driver
    /// `/proc/ultar/geraet` says which driver operates each graphics card.
    fn class_probe(&self, device: &DeviceDescriptor, class: DriverClass, driver: &str) -> bool {
        let (DriverClass::Display, Some(info)) = (class, &self.info) else {
            return true;
        };
        let HardwareId::Pci { vendor, device, .. } = device.primary else {
            return true;
        };
        match info
            .graphics
            .iter()
            .find(|c| (c.vendor, c.device) == (vendor, device))
        {
            Some(card) => card.driver.as_deref() == Some(driver),
            None => true,
        }
    }
}

fn failure_of(detail: &LoadDetail) -> Result<(), LoadError> {
    Err(match detail {
        LoadDetail::Loaded { .. } => return Ok(()),
        LoadDetail::PackageMissing => LoadError::Failed(LoadFailure::PackageMissing),
        LoadDetail::PackageRejected(_)
        | LoadDetail::WrongPackage
        | LoadDetail::BadActivationRecord => LoadError::Failed(LoadFailure::PackageRejected),
        LoadDetail::NoActivationRecord | LoadDetail::NotBound { .. } => {
            LoadError::Failed(LoadFailure::Unsupported)
        }
        LoadDetail::ProbeFailed(_) => LoadError::Failed(LoadFailure::InitFailed),
        LoadDetail::DeviceGone => LoadError::DeviceGone,
    })
}

impl DriverLoader for Builtin {
    fn load(&mut self, device: &DeviceDescriptor, driver: &Candidate) -> Result<(), LoadError> {
        let mut inner = self.lock();
        let detail = inner.confirm(device, driver);
        let result = failure_of(&detail);
        inner.remember(&device.address, DriverKey::of(driver), detail);
        result
    }

    /// Nothing to do: the kernel has no interface to deactivate a built-in driver, and a
    /// stage-A package holds no state of its own.
    fn unload(&mut self, _device: &DeviceDescriptor, _driver: &Candidate) {}

    fn can_switch(&mut self, device: &DeviceDescriptor, target: &Candidate) -> bool {
        let inner = self.lock();
        matches!(inner.confirm(device, target), LoadDetail::Loaded { .. })
    }
}

impl HealthCheck for Builtin {
    fn check(&mut self, device: &DeviceDescriptor, driver: &Candidate) -> HealthVerdict {
        let inner = self.lock();
        let key = (device.address.clone(), DriverKey::of(driver));
        let Some(LoadDetail::Loaded { kernel_driver }) = inner.details.get(&key) else {
            return HealthVerdict::Unhealthy(HealthFailure::NoResponse);
        };
        match inner.drivers.binding(&device.address) {
            BindState::Bound { driver: bound } if &bound == kernel_driver => {}
            BindState::Unknown => return HealthVerdict::DeviceGone,
            _ => return HealthVerdict::Unhealthy(HealthFailure::NoResponse),
        }
        if !inner.class_probe(device, driver.class, kernel_driver) {
            return HealthVerdict::Unhealthy(HealthFailure::SelfTestFailed);
        }
        HealthVerdict::Healthy(vec![Capability::new(
            class_capability(driver.class),
            "present",
        )])
    }
}
