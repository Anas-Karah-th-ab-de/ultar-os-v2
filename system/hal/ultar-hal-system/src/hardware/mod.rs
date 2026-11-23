//! Hardware inventory of the running system as [`DeviceDescriptor`]s for driver matching.
//!
//! Both kernels describe their devices in the Linux sysfs layout ([`sysfs`]): every PCI
//! function and USB device with complete ids and class codes, below an injectable root
//! directory. The Ultar kernel's `/proc/ultar/geraet` ([`device_info`]) adds the machine
//! model and which driver runs each graphics card.

pub mod device_info;
pub mod sysfs;

use device_info::KernelDeviceInfo;
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use ultar_driverpkg::{DeviceDescriptor, HardwareId};

/// Largest text file read from the system description.
pub const MAX_FILE: u64 = 1024 * 1024;

/// Why the hardware could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardwareError {
    /// No sysfs device tree exists below the root.
    NoSource,
    /// A file or directory exists but cannot be read.
    Io {
        /// The path.
        path: String,
        /// The I/O error kind.
        kind: std::io::ErrorKind,
    },
    /// A directory holds more entries than a machine can have devices.
    TooManyDevices,
}

impl HardwareError {
    /// Stable message key.
    pub fn key(&self) -> &'static str {
        match self {
            Self::NoSource => "hal.hardware.no-source",
            Self::Io { .. } => "hal.hardware.io",
            Self::TooManyDevices => "hal.hardware.too-many-devices",
        }
    }
}

impl fmt::Display for HardwareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, kind } => write!(f, "{}: {path}: {kind:?}", self.key()),
            _ => f.write_str(self.key()),
        }
    }
}

impl std::error::Error for HardwareError {}

/// Reads a text file of at most [`MAX_FILE`] bytes (longer files are cut); `None` when it
/// does not exist. Invalid UTF-8 is replaced, never rejected.
pub(crate) fn read_capped(path: &Path) -> Result<Option<String>, HardwareError> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(HardwareError::Io {
                path: path.display().to_string(),
                kind: error.kind(),
            })
        }
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE)
        .read_to_end(&mut bytes)
        .map_err(|error| HardwareError::Io {
            path: path.display().to_string(),
            kind: error.kind(),
        })?;
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// Which sources contributed to an inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sources {
    /// `/sys/bus/pci/devices` was read.
    pub sysfs_pci: bool,
    /// `/sys/bus/usb/devices` was read.
    pub sysfs_usb: bool,
}

/// The devices of the running system.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inventory {
    /// All devices, ordered by address.
    pub devices: Vec<DeviceDescriptor>,
    /// Machine model, when known.
    pub machine: Option<String>,
    /// The sources that were present.
    pub sources: Sources,
}

/// Reads the hardware description below a root directory.
#[derive(Debug, Clone)]
pub struct HardwareReader {
    root: PathBuf,
}

impl HardwareReader {
    /// Reader for the running system.
    pub fn running() -> Self {
        Self::at(Path::new("/"))
    }

    /// Reader below `root` (tests, inspection of a mounted system).
    pub fn at(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// The root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The parsed Ultar device description; `None` when the file does not exist.
    pub fn device_info(&self) -> Result<Option<KernelDeviceInfo>, HardwareError> {
        let text = read_capped(&self.root.join("proc/ultar/geraet"))?;
        Ok(text.map(|text| KernelDeviceInfo::parse(&text)))
    }

    /// Enumerates the devices of the system from all available sources.
    pub fn inventory(&self) -> Result<Inventory, HardwareError> {
        let pci = sysfs::read_pci(&self.root)?;
        let usb = sysfs::read_usb(&self.root)?;
        if pci.is_none() && usb.is_none() {
            return Err(HardwareError::NoSource);
        }
        let machine = match self.device_info()?.and_then(|info| info.model) {
            Some(model) => Some(model),
            None => sysfs::machine(&self.root)?,
        };
        let mut inventory = Inventory {
            machine,
            sources: Sources {
                sysfs_pci: pci.is_some(),
                sysfs_usb: usb.is_some(),
            },
            ..Inventory::default()
        };
        let mut devices: BTreeMap<String, DeviceDescriptor> = BTreeMap::new();
        for device in pci.iter().flatten() {
            let id = HardwareId::Pci {
                vendor: device.vendor,
                device: device.device,
                class: device.class,
                subsys: device.subsys,
            };
            let address = sysfs::pci_address(&device.name);
            devices.insert(address.clone(), DeviceDescriptor::new(address, id));
        }
        for device in usb.iter().flatten() {
            let id = HardwareId::Usb {
                vendor: device.vendor,
                product: device.product,
                class: device.class,
            };
            let address = format!("usb:{}", device.name);
            devices.insert(address.clone(), DeviceDescriptor::new(address, id));
        }
        inventory.devices = devices
            .into_values()
            .map(|mut device| {
                device.machine.clone_from(&inventory.machine);
                device
            })
            .collect();
        Ok(inventory)
    }
}
