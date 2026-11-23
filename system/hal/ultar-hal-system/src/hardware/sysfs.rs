//! The Linux-compatible sysfs device tree (`/sys/bus/pci`, `/sys/bus/usb`, DMI).
//!
//! The Ultar kernel and the transitional Linux device kernels both expose it; the Ultar
//! kernel names its own extensions `ultar_*` (see [`crate::kernel_drivers`]).

use super::{read_capped, HardwareError};
use std::fs;
use std::path::Path;

/// Most entries read from one sysfs directory.
pub const MAX_ENTRIES: usize = 4096;

/// One PCI function from sysfs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysfsPci {
    /// Directory name, e.g. `0000:00:1f.6`.
    pub name: String,
    /// Vendor id.
    pub vendor: u16,
    /// Device id.
    pub device: u16,
    /// 24-bit class code.
    pub class: u32,
    /// Subsystem vendor and device.
    pub subsys: Option<(u16, u16)>,
    /// Name of the bound kernel driver.
    pub driver: Option<String>,
}

/// One USB device from sysfs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysfsUsb {
    /// Directory name, e.g. `1-14` or `1-2.1`.
    pub name: String,
    /// Vendor id.
    pub vendor: u16,
    /// Product id.
    pub product: u16,
    /// Device class, subclass, protocol (`0` when defined per interface).
    pub class: u32,
    /// Kernel driver of the first interface that has one.
    pub driver: Option<String>,
}

fn io_error(path: &Path, error: &std::io::Error) -> HardwareError {
    HardwareError::Io {
        path: path.display().to_string(),
        kind: error.kind(),
    }
}

/// Sorted entry names of a directory; `None` when it does not exist.
pub(crate) fn list(dir: &Path) -> Result<Option<Vec<String>>, HardwareError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(dir, &error)),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| io_error(dir, &error))?;
        if names.len() >= MAX_ENTRIES {
            return Err(HardwareError::TooManyDevices);
        }
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(Some(names))
}

fn hex_file(path: &Path) -> Result<Option<u32>, HardwareError> {
    let Some(text) = read_capped(path)? else {
        return Ok(None);
    };
    let text = text.trim();
    let digits = text.strip_prefix("0x").unwrap_or(text);
    Ok(u32::from_str_radix(digits, 16).ok())
}

fn hex16_file(path: &Path) -> Result<Option<u16>, HardwareError> {
    Ok(hex_file(path)?.and_then(|value| u16::try_from(value).ok()))
}

/// Target name of a `driver` link.
pub(crate) fn driver_link(device_dir: &Path) -> Option<String> {
    let target = fs::read_link(device_dir.join("driver")).ok()?;
    Some(target.file_name()?.to_string_lossy().into_owned())
}

/// PCI functions below `root`; `None` when the tree does not exist.
pub fn read_pci(root: &Path) -> Result<Option<Vec<SysfsPci>>, HardwareError> {
    let base = root.join("sys/bus/pci/devices");
    let Some(names) = list(&base)? else {
        return Ok(None);
    };
    let mut devices = Vec::new();
    for name in names {
        let dir = base.join(&name);
        let (Some(vendor), Some(device), Some(class)) = (
            hex16_file(&dir.join("vendor"))?,
            hex16_file(&dir.join("device"))?,
            hex_file(&dir.join("class"))?,
        ) else {
            continue;
        };
        let subsys = match (
            hex16_file(&dir.join("subsystem_vendor"))?,
            hex16_file(&dir.join("subsystem_device"))?,
        ) {
            (Some(vendor), Some(device)) if (vendor, device) != (0, 0) => Some((vendor, device)),
            _ => None,
        };
        devices.push(SysfsPci {
            driver: driver_link(&dir),
            name,
            vendor,
            device,
            class: class & 0x00ff_ffff,
            subsys,
        });
    }
    Ok(Some(devices))
}

fn usb_interface_driver(base: &Path, names: &[String], device: &str) -> Option<String> {
    let prefix = format!("{device}:");
    names
        .iter()
        .filter(|name| name.starts_with(&prefix))
        .find_map(|name| driver_link(&base.join(name)))
}

/// USB devices below `root` (no root hubs, no interface entries); `None` when the tree
/// does not exist.
pub fn read_usb(root: &Path) -> Result<Option<Vec<SysfsUsb>>, HardwareError> {
    let base = root.join("sys/bus/usb/devices");
    let Some(names) = list(&base)? else {
        return Ok(None);
    };
    let mut devices = Vec::new();
    for name in &names {
        if name.contains(':') || name.starts_with("usb") {
            continue;
        }
        let dir = base.join(name);
        let (Some(vendor), Some(product)) = (
            hex16_file(&dir.join("idVendor"))?,
            hex16_file(&dir.join("idProduct"))?,
        ) else {
            continue;
        };
        let part = |file: &str| -> Result<u32, HardwareError> {
            Ok(hex_file(&dir.join(file))?.unwrap_or(0) & 0xff)
        };
        let class = (part("bDeviceClass")? << 16)
            | (part("bDeviceSubClass")? << 8)
            | part("bDeviceProtocol")?;
        devices.push(SysfsUsb {
            name: name.clone(),
            vendor,
            product,
            class,
            driver: usb_interface_driver(&base, &names, name),
        });
    }
    Ok(Some(devices))
}

/// Machine model from DMI (`product_name`).
pub fn machine(root: &Path) -> Result<Option<String>, HardwareError> {
    let text = read_capped(&root.join("sys/class/dmi/id/product_name"))?;
    Ok(text
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty()))
}

/// The address used for a sysfs PCI entry: `pci:bb:dd.f`, with the domain kept when it is not 0.
pub fn pci_address(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.strip_prefix("0000:") {
        Some(slot) => format!("pci:{slot}"),
        None => format!("pci:{lower}"),
    }
}
