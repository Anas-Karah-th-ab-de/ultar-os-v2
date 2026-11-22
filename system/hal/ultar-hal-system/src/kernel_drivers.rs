//! Built-in kernel drivers of the running system and what they are bound to.
//!
//! **Ultar kernel (0.4):** there is no driver file below `/proc/ultar`. The bind result of
//! every device a built-in driver matched is printed once at boot (`[treiber] …`, see
//! [`crate::hardware::kernel_log`]); the complete driver register is only in the log after
//! the console command `treiber` was run. The kernel has **no interface to bind, unbind,
//! enable or disable a built-in driver** – `driver.rs::bind_all` runs once at boot and
//! neither `/proc/ultar` nor a system call changes its result. [`ControlInterface::None`]
//! says so; nothing here pretends otherwise.
//!
//! **Linux-compatible sysfs:** `/sys/bus/pci/drivers/<name>/` lists the drivers, the
//! `driver` links of the devices give the bindings.

use crate::hardware::kernel_log::{KernelLog, LogBinding};
use crate::hardware::{read_capped, sysfs, HardwareError, HardwareReader};
use std::collections::BTreeMap;
use std::path::Path;

/// The driver ABI name of the Ultar kernel. The kernel exposes no ABI number yet, so this
/// is the one ABI that exists; it must become a kernel-reported value before a second one appears.
pub const ULTAR_KERNEL_ABI: &str = "ultar-kern-abi-1";

/// Relation between a device and the kernel's built-in drivers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindState {
    /// A kernel driver operates the device.
    Bound {
        /// Kernel driver name.
        driver: String,
    },
    /// A kernel driver matched the device but its probe failed.
    Failed {
        /// Kernel driver name, when the kernel named it.
        driver: Option<String>,
        /// The kernel's error text (technical).
        error: String,
    },
    /// The device is present and no kernel driver operates it.
    Unbound,
    /// The system description does not mention a device at this address.
    Unknown,
}

/// One driver built into the kernel.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KernelDriver {
    /// Driver name.
    pub name: String,
    /// `hand` or `werkstatt` (Ultar kernel), `None` when unknown.
    pub origin: Option<String>,
    /// Addresses of the devices the driver operates.
    pub bound: Vec<String>,
    /// Addresses of the devices on which its probe failed.
    pub failed: Vec<String>,
}

/// How user space can change which built-in driver operates a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlInterface {
    /// The kernel offers no such interface (Ultar kernel 0.4).
    None,
    /// `bind`/`unbind` files exist below `/sys/bus/pci/drivers/<name>/` (Linux).
    SysfsBind,
}

/// Snapshot of the built-in drivers and their bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelDrivers {
    drivers: BTreeMap<String, KernelDriver>,
    bindings: BTreeMap<String, BindState>,
    /// `true` when the complete driver register was readable; otherwise only drivers that
    /// matched a device are known and a missing name proves nothing.
    pub register_complete: bool,
    /// The interface for changing bindings.
    pub control: ControlInterface,
}

impl KernelDrivers {
    /// Reads the snapshot of the system below the reader's root.
    pub fn read(reader: &HardwareReader) -> Result<Self, HardwareError> {
        let mut snapshot = Self {
            drivers: BTreeMap::new(),
            bindings: BTreeMap::new(),
            register_complete: false,
            control: ControlInterface::None,
        };
        if let Some(log) = reader.kernel_log()? {
            snapshot.add_log(&log);
        }
        snapshot.add_sysfs(reader.root())?;
        Ok(snapshot)
    }

    fn driver_mut(&mut self, name: &str) -> &mut KernelDriver {
        self.drivers
            .entry(name.to_string())
            .or_insert_with(|| KernelDriver {
                name: name.to_string(),
                ..KernelDriver::default()
            })
    }

    fn bind(&mut self, address: String, driver: &str) {
        let entry = self.driver_mut(driver);
        if !entry.bound.contains(&address) {
            entry.bound.push(address.clone());
        }
        self.bindings.insert(
            address,
            BindState::Bound {
                driver: driver.to_string(),
            },
        );
    }

    fn add_log(&mut self, log: &KernelLog) {
        for row in &log.drivers {
            self.driver_mut(&row.name).origin = Some(row.origin.clone());
        }
        self.register_complete = !log.drivers.is_empty();
        for device in &log.pci {
            let address = format!("pci:{}", device.slot);
            match &device.binding {
                Some(LogBinding::Bound { driver, origin }) => {
                    if origin.is_some() {
                        self.driver_mut(driver).origin.clone_from(origin);
                    }
                    self.bind(address, driver);
                }
                Some(LogBinding::Failed { driver, error }) => {
                    if let Some(driver) = driver {
                        self.driver_mut(driver).failed.push(address.clone());
                    }
                    let state = BindState::Failed {
                        driver: driver.clone(),
                        error: error.clone(),
                    };
                    self.bindings.insert(address, state);
                }
                Some(LogBinding::Unbound) | None => {
                    self.bindings.insert(address, BindState::Unbound);
                }
            }
        }
        for device in &log.usb {
            let address = format!("usb:{}-{}", device.controller, device.port);
            self.bindings.insert(address, BindState::Unbound);
        }
    }

    fn add_sysfs(&mut self, root: &Path) -> Result<(), HardwareError> {
        let register = root.join("sys/bus/pci/drivers");
        if let Some(names) = sysfs::list(&register)? {
            self.register_complete = true;
            for name in names {
                let dir = register.join(&name);
                if dir.join("bind").exists() && dir.join("unbind").exists() {
                    self.control = ControlInterface::SysfsBind;
                }
                self.driver_mut(&name);
            }
        }
        for device in sysfs::read_pci(root)?.into_iter().flatten() {
            let address = sysfs::pci_address(&device.name);
            match &device.driver {
                Some(driver) => self.bind(address, driver),
                None => {
                    self.bindings.insert(address, BindState::Unbound);
                }
            }
        }
        for device in sysfs::read_usb(root)?.into_iter().flatten() {
            let address = format!("usb:{}", device.name);
            match &device.driver {
                Some(driver) => self.bind(address, driver),
                None => {
                    self.bindings.insert(address, BindState::Unbound);
                }
            }
        }
        Ok(())
    }

    /// All known drivers, ordered by name.
    pub fn drivers(&self) -> impl Iterator<Item = &KernelDriver> {
        self.drivers.values()
    }

    /// One driver by name.
    pub fn driver(&self, name: &str) -> Option<&KernelDriver> {
        self.drivers.get(name)
    }

    /// Whether the kernel has a driver of this name: `Some(false)` only when the complete
    /// register was readable, `None` when the question cannot be answered.
    pub fn exists(&self, name: &str) -> Option<bool> {
        match (self.drivers.contains_key(name), self.register_complete) {
            (true, _) => Some(true),
            (false, true) => Some(false),
            (false, false) => None,
        }
    }

    /// The bind state of the device at `address`.
    pub fn binding(&self, address: &str) -> BindState {
        self.bindings
            .get(address)
            .cloned()
            .unwrap_or(BindState::Unknown)
    }

    /// Number of devices the snapshot knows.
    pub fn device_count(&self) -> usize {
        self.bindings.len()
    }
}

/// The driver ABI of the running kernel as used in driver manifests (`kernel_abi`):
/// [`ULTAR_KERNEL_ABI`] on the Ultar kernel, `linux-<release>` on a Linux kernel,
/// `None` when neither `/proc/version` nor the release file is readable.
pub fn kernel_abi(root: &Path) -> Result<Option<String>, HardwareError> {
    if let Some(version) = read_capped(&root.join("proc/version"))? {
        // procfs.rs: "Linux version 6.8.0-ultar-kern (Ultar-Kern 0.4, KM Code Craft) #1"
        if version.contains("Ultar-Kern") {
            return Ok(Some(ULTAR_KERNEL_ABI.to_string()));
        }
    }
    let release = read_capped(&root.join("proc/sys/kernel/osrelease"))?;
    Ok(release
        .map(|release| release.trim().to_string())
        .filter(|release| !release.is_empty())
        .map(|release| format!("linux-{release}")))
}
