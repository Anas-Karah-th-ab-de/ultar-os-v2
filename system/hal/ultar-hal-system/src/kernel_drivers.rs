//! Built-in kernel drivers of the running system, what they are bound to, and the control
//! files that change it.
//!
//! Both kernels describe this in the Linux sysfs layout:
//!
//! * `/sys/bus/pci/drivers/<name>/` – the driver register; `bind`/`unbind` take the name of a
//!   PCI function (`0000:00:04.0`); the `driver` link of a device names its driver;
//! * the Ultar kernel adds `ultar_origin` (`hand`, `werkstatt`), `ultar_unbind` (`supported`
//!   or `unsupported: <reason>` – a driver without a teardown path is never unbound) and
//!   `ultar_enabled` per driver, and `ultar_probe` (`ok`, `none`, `failed: <driver> <text>`)
//!   and `ultar_health` (`key=value` lines) per device;
//! * `/sys/kernel/ultar/driver_abi` is the Ultar kernel's driver ABI number.
//!
//! Writing a control file needs the kernel right `LOAD_DRIVER` (Ultar) or root (Linux).

use crate::hardware::{read_capped, sysfs, HardwareError, HardwareReader};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Prefix of the driver ABI names of the Ultar kernel (`ultar-kern-abi-<number>`).
pub const ULTAR_ABI_PREFIX: &str = "ultar-kern-abi-";
/// The driver ABI name of Ultar kernels that report number 1.
pub const ULTAR_KERNEL_ABI: &str = "ultar-kern-abi-1";

/// Relation between a device and the kernel's built-in drivers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindState {
    /// A kernel driver operates the device.
    Bound {
        /// Kernel driver name.
        driver: String,
    },
    /// A kernel driver's probe or teardown failed on the device.
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
    /// Whether the driver can be unbound (it has a teardown path); `None` when not reported.
    pub unbind: Option<bool>,
    /// Whether the driver is turned on; `None` when not reported.
    pub enabled: Option<bool>,
}

/// How user space can change which built-in driver operates a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlInterface {
    /// The kernel offers no such interface.
    None,
    /// `bind`/`unbind` files exist below `/sys/bus/pci/drivers/<name>/`.
    SysfsBind,
}

/// Snapshot of the built-in drivers and their bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelDrivers {
    root: PathBuf,
    drivers: BTreeMap<String, KernelDriver>,
    bindings: BTreeMap<String, BindState>,
    health: BTreeMap<String, String>,
    candidates: BTreeMap<String, Vec<String>>,
    /// `true` when the driver register was readable; otherwise a missing name proves nothing.
    pub register_complete: bool,
    /// The interface for changing bindings.
    pub control: ControlInterface,
}

/// `pci:00:04.0` → `0000:00:04.0`; `None` for other buses.
pub fn sysfs_pci_name(address: &str) -> Option<String> {
    let slot = address.strip_prefix("pci:")?;
    Some(if slot.matches(':').count() >= 2 {
        slot.to_string()
    } else {
        format!("0000:{slot}")
    })
}

fn first_line(text: Option<String>) -> Option<String> {
    text.map(|t| t.lines().next().unwrap_or("").trim().to_string())
}

/// `failed: <driver> <text>` of `ultar_probe`.
fn probe_failure(text: &str) -> Option<BindState> {
    let rest = text.strip_prefix("failed: ")?;
    let (driver, error) = rest.split_once(' ').unwrap_or((rest, ""));
    Some(BindState::Failed {
        driver: (driver != "-").then(|| driver.to_string()),
        error: error.to_string(),
    })
}

impl KernelDrivers {
    /// Reads the snapshot of the system below the reader's root.
    pub fn read(reader: &HardwareReader) -> Result<Self, HardwareError> {
        let mut snapshot = Self {
            root: reader.root().to_path_buf(),
            drivers: BTreeMap::new(),
            bindings: BTreeMap::new(),
            health: BTreeMap::new(),
            candidates: BTreeMap::new(),
            register_complete: false,
            control: ControlInterface::None,
        };
        snapshot.read_register()?;
        snapshot.read_pci()?;
        snapshot.read_usb()?;
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

    fn read_register(&mut self) -> Result<(), HardwareError> {
        let register = self.root.join("sys/bus/pci/drivers");
        let Some(names) = sysfs::list(&register)? else {
            return Ok(());
        };
        self.register_complete = true;
        for name in names {
            let dir = register.join(&name);
            if dir.join("bind").exists() && dir.join("unbind").exists() {
                self.control = ControlInterface::SysfsBind;
            }
            let origin = first_line(read_capped(&dir.join("ultar_origin"))?);
            let unbind =
                first_line(read_capped(&dir.join("ultar_unbind"))?).map(|t| t == "supported");
            let enabled = first_line(read_capped(&dir.join("ultar_enabled"))?).map(|t| t == "1");
            let entry = self.driver_mut(&name);
            entry.origin = origin;
            entry.unbind = unbind;
            entry.enabled = enabled;
        }
        Ok(())
    }

    fn read_pci(&mut self) -> Result<(), HardwareError> {
        let base = self.root.join("sys/bus/pci/devices");
        for device in sysfs::read_pci(&self.root)?.into_iter().flatten() {
            let address = sysfs::pci_address(&device.name);
            let dir = base.join(&device.name);
            if let Some(health) = read_capped(&dir.join("ultar_health"))? {
                self.health.insert(address.clone(), health);
            }
            if let Some(list) = read_capped(&dir.join("ultar_drivers"))? {
                let names = list
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect();
                self.candidates.insert(address.clone(), names);
            }
            if let Some(driver) = &device.driver {
                self.bind(address, driver);
                continue;
            }
            let probe = first_line(read_capped(&dir.join("ultar_probe"))?).unwrap_or_default();
            match probe_failure(&probe) {
                Some(BindState::Failed { driver, error }) => {
                    if let Some(name) = &driver {
                        self.driver_mut(name).failed.push(address.clone());
                    }
                    self.bindings
                        .insert(address, BindState::Failed { driver, error });
                }
                _ => {
                    self.bindings.insert(address, BindState::Unbound);
                }
            }
        }
        Ok(())
    }

    fn read_usb(&mut self) -> Result<(), HardwareError> {
        for device in sysfs::read_usb(&self.root)?.into_iter().flatten() {
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

    /// Whether the kernel has a driver of this name: `Some(false)` only when the register
    /// was readable, `None` when the question cannot be answered.
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

    /// The kernel's health lines of the device (`probe=ok`, `link=up`, …), if reported.
    pub fn health(&self, address: &str) -> Option<&str> {
        self.health.get(address).map(String::as_str)
    }

    /// The kernel drivers that match the device (`ultar_drivers`), if reported.
    pub fn candidates(&self, address: &str) -> Option<&[String]> {
        self.candidates.get(address).map(Vec::as_slice)
    }

    /// Number of devices the snapshot knows.
    pub fn device_count(&self) -> usize {
        self.bindings.len()
    }

    fn control_write(&self, relative: &str, value: &str) -> Result<(), HardwareError> {
        let path = self.root.join(relative);
        std::fs::write(&path, value).map_err(|error| HardwareError::Io {
            path: path.display().to_string(),
            kind: error.kind(),
        })
    }

    /// Binds `driver` to the PCI device at `address` (`/sys/bus/pci/drivers/<driver>/bind`).
    /// Read the snapshot again to see the result.
    pub fn bind_device(&self, driver: &str, address: &str) -> Result<(), HardwareError> {
        let name = sysfs_pci_name(address).ok_or(HardwareError::NoSource)?;
        self.control_write(&format!("sys/bus/pci/drivers/{driver}/bind"), &name)
    }

    /// Unbinds the driver of the PCI device at `address`. The kernel refuses a driver without
    /// a teardown path (`Unsupported`) instead of pretending.
    pub fn unbind_device(&self, address: &str) -> Result<(), HardwareError> {
        let BindState::Bound { driver } = self.binding(address) else {
            return Ok(());
        };
        let name = sysfs_pci_name(address).ok_or(HardwareError::NoSource)?;
        self.control_write(&format!("sys/bus/pci/drivers/{driver}/unbind"), &name)
    }
}

/// The kernel's device event counter (`/sys/kernel/uevent_seqnum`): it changes whenever a
/// device comes, goes or changes its binding. `None` when the kernel does not report it.
pub fn uevent_seqnum(root: &Path) -> Result<Option<u64>, HardwareError> {
    Ok(
        first_line(read_capped(&root.join("sys/kernel/uevent_seqnum"))?)
            .and_then(|t| t.parse().ok()),
    )
}

/// The driver ABI of the running kernel as used in driver manifests (`kernel_abi`):
/// `ultar-kern-abi-<n>` from `/sys/kernel/ultar/driver_abi` on the Ultar kernel,
/// `linux-<release>` on a Linux kernel, `None` when neither is readable.
pub fn kernel_abi(root: &Path) -> Result<Option<String>, HardwareError> {
    if let Some(number) = first_line(read_capped(&root.join("sys/kernel/ultar/driver_abi"))?) {
        if let Ok(n) = number.parse::<u32>() {
            return Ok(Some(format!("{ULTAR_ABI_PREFIX}{n}")));
        }
    }
    let release = read_capped(&root.join("proc/sys/kernel/osrelease"))?;
    Ok(release
        .map(|release| release.trim().to_string())
        .filter(|release| !release.is_empty())
        .map(|release| format!("linux-{release}")))
}
