//! The driver depot of the installation medium: building the signed index from a
//! directory of packages, resolving a machine's devices and selecting what to install.

use crate::hardware::{DeviceDescriptor, HardwareId};
use crate::index::{DriverIndex, IndexEntry, IndexError, INDEX_NAME, INDEX_SIGNATURE_NAME};
use crate::keys::{PrivateKey, TrustedKeys};
use crate::matching::{candidates, Candidate};
use crate::package::{Limits, EXTENSION};
use std::collections::BTreeSet;
use std::path::Path;

fn io_error(path: &Path, error: &std::io::Error) -> IndexError {
    IndexError::Io {
        path: path.display().to_string(),
        kind: error.kind(),
    }
}

/// Verifies every `*.utr` file in `dir` and builds the index. Any rejected package aborts the scan.
pub fn scan_directory(
    dir: &Path,
    trusted: &TrustedKeys,
    limits: &Limits,
) -> Result<DriverIndex, IndexError> {
    let mut paths = Vec::new();
    for item in std::fs::read_dir(dir).map_err(|e| io_error(dir, &e))? {
        let path = item.map_err(|e| io_error(dir, &e))?.path();
        if path.is_file() && path.extension().is_some_and(|x| x == EXTENSION) {
            paths.push(path);
        }
    }
    paths.sort();
    let mut entries = Vec::with_capacity(paths.len());
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = std::fs::read(&path).map_err(|e| io_error(&path, &e))?;
        entries.push(IndexEntry::from_package(&name, &bytes, trusted, limits)?);
    }
    DriverIndex::new(entries)
}

/// Scans `dir`, then writes `index.toml` and `index.toml.sig` into it.
pub fn write_signed_index(
    dir: &Path,
    key: &PrivateKey,
    trusted: &TrustedKeys,
    limits: &Limits,
) -> Result<DriverIndex, IndexError> {
    // the serial: unchanged for the same packages (a rebuild gives byte-identical output), else
    // seconds since 1970 and above the index already there (devices refuse a lower one)
    let before = std::fs::read_to_string(dir.join(INDEX_NAME))
        .ok()
        .and_then(|text| DriverIndex::parse(&text).ok());
    let scanned = scan_directory(dir, trusted, limits)?;
    let serial = match &before {
        Some(old) if old.entries() == scanned.entries() => old.serial(),
        _ => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            now.max(before.map_or(0, |old| old.serial() + 1))
        }
    };
    let index = scanned.with_serial(serial);
    let text = index.to_toml()?;
    let index_path = dir.join(INDEX_NAME);
    std::fs::write(&index_path, &text).map_err(|e| io_error(&index_path, &e))?;
    let signature_path = dir.join(INDEX_SIGNATURE_NAME);
    std::fs::write(&signature_path, DriverIndex::sign(&text, key))
        .map_err(|e| io_error(&signature_path, &e))?;
    Ok(index)
}

/// Loads `index.toml` from a depot directory and verifies its signature.
pub fn load_signed_index(dir: &Path, trusted: &TrustedKeys) -> Result<DriverIndex, IndexError> {
    let index_path = dir.join(INDEX_NAME);
    let index = std::fs::read(&index_path).map_err(|e| io_error(&index_path, &e))?;
    let signature = match std::fs::read(dir.join(INDEX_SIGNATURE_NAME)) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(io_error(&dir.join(INDEX_SIGNATURE_NAME), &e)),
    };
    DriverIndex::parse_verified(&index, signature.as_deref(), trusted)
}

/// The resolution for one device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevicePlan {
    /// The device.
    pub device: DeviceDescriptor,
    /// The best driver, `None` when no driver fits.
    pub chosen: Option<Candidate>,
    /// Further fitting drivers, best first.
    pub alternatives: Vec<Candidate>,
}

/// A request to the driver workshop to create a blueprint for an unsupported device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlueprintRequest {
    /// Address of the device on the machine.
    pub address: String,
    /// All identifiers of the device.
    pub ids: Vec<HardwareId>,
    /// Inventory label, if any.
    pub label: Option<String>,
    /// Machine model, if known.
    pub machine: Option<String>,
}

/// The result of matching all devices of a machine against a depot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// One entry per device, in input order.
    pub devices: Vec<DevicePlan>,
    depot_files: Vec<String>,
}

impl Plan {
    /// Devices for which the depot has no driver.
    pub fn driverless(&self) -> Vec<&DeviceDescriptor> {
        self.devices
            .iter()
            .filter(|d| d.chosen.is_none())
            .map(|d| &d.device)
            .collect()
    }

    /// Workshop blueprint requests for all driverless devices.
    pub fn blueprint_requests(&self) -> Vec<BlueprintRequest> {
        self.driverless()
            .into_iter()
            .map(|device| BlueprintRequest {
                address: device.address.clone(),
                ids: device.ids().cloned().collect(),
                label: device.label.clone(),
                machine: device.machine.clone(),
            })
            .collect()
    }

    /// The plan entry of the device at `address`.
    pub fn device(&self, address: &str) -> Option<&DevicePlan> {
        self.devices.iter().find(|d| d.device.address == address)
    }
}

/// Matches every device against the index: chosen driver, alternatives, driverless devices.
pub fn resolve(devices: &[DeviceDescriptor], index: &DriverIndex, kernel_abi: &str) -> Plan {
    let plans = devices
        .iter()
        .map(|device| {
            let mut found = candidates(device, index, kernel_abi).into_iter();
            DevicePlan {
                device: device.clone(),
                chosen: found.next(),
                alternatives: found.collect(),
            }
        })
        .collect();
    Plan {
        devices: plans,
        depot_files: index.entries().iter().map(|e| e.file.clone()).collect(),
    }
}

/// The files to copy from the depot to the target disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallSelection {
    /// Package files, sorted and without duplicates.
    pub packages: Vec<String>,
    /// The signed index files; always copied so the installed system can verify its drivers
    /// (limit the loaded index with [`DriverIndex::retain_files`] to the copied packages).
    pub index_files: Vec<String>,
}

/// Selects the packages to install: only the chosen drivers, or with `all` the whole depot
/// (portable installations that must start on other machines).
pub fn select_for_install(plan: &Plan, all: bool) -> InstallSelection {
    let packages: BTreeSet<String> = if all {
        plan.depot_files.iter().cloned().collect()
    } else {
        plan.devices
            .iter()
            .filter_map(|d| d.chosen.as_ref())
            .map(|c| c.file.clone())
            .collect()
    };
    InstallSelection {
        packages: packages.into_iter().collect(),
        index_files: vec![INDEX_NAME.to_string(), INDEX_SIGNATURE_NAME.to_string()],
    }
}
