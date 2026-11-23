//! System source of the device profile (area 3, HAL).
//!
//! `ultar-profile` is pure logic and never touches hardware paths. This crate is the one
//! place that reads the description of the running system – kernel command line, the
//! device description of the Ultar kernel, SMBIOS chassis type, device tree, power
//! supplies and memory – and hands it to `ultar-profile` through [`ProfileSource`].
//!
//! Both the Ultar kernel and the transitional Linux device kernels expose these files
//! under the same names, so one source serves both.
//!
//! [`hardware`] enumerates the devices of the running system for driver matching and
//! [`kernel_drivers`] reads which built-in kernel drivers exist and what they operate.
//! This crate is the only place where hardware paths (`/proc/ultar/…`, `/sys/…`) appear.

#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod hardware;
pub mod kernel_drivers;
pub mod seat_nodes;

pub use hardware::{HardwareError, HardwareReader, Inventory};
pub use kernel_drivers::{kernel_abi, uevent_seqnum, BindState, ControlInterface, KernelDrivers};

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use ultar_profile::{Capabilities, Capability, DeviceProfile, Edition, ProfileSource};

/// Default location of the edition file written by the image build.
pub const EDITION_FILE: &str = "/etc/ultar/edition";

/// Reads the profile inputs from the file system below a root directory.
///
/// The root is `/` on a device; tests point it at a prepared directory tree.
#[derive(Debug, Clone)]
pub struct SystemSource {
    root: PathBuf,
    edition_env: Option<String>,
    edition_file: PathBuf,
}

impl SystemSource {
    /// Source for the running system, honouring `ULTAR_EDITION` and `ULTAR_EDITION_FILE`.
    pub fn running() -> Self {
        let edition_file =
            std::env::var("ULTAR_EDITION_FILE").unwrap_or_else(|_| EDITION_FILE.to_string());
        SystemSource {
            root: PathBuf::from("/"),
            edition_env: std::env::var("ULTAR_EDITION").ok(),
            edition_file: PathBuf::from(edition_file),
        }
    }

    /// Source reading below `root` (for tests and for inspecting an installed system).
    pub fn at(root: &Path) -> Self {
        SystemSource {
            root: root.to_path_buf(),
            edition_env: None,
            edition_file: root.join("etc/ultar/edition"),
        }
    }

    /// Sets the edition requested through the environment.
    pub fn with_edition_env(mut self, value: &str) -> Self {
        self.edition_env = Some(value.to_string());
        self
    }

    fn read(&self, relative: &str) -> Option<String> {
        fs::read_to_string(self.root.join(relative)).ok()
    }

    fn has_battery(&self) -> bool {
        let Ok(entries) = fs::read_dir(self.root.join("sys/class/power_supply")) else {
            return false;
        };
        entries.flatten().any(|entry| {
            fs::read_to_string(entry.path().join("type"))
                .map(|kind| kind.trim() == "Battery")
                .unwrap_or(false)
        })
    }

    fn memory_mb(&self) -> u32 {
        let Some(meminfo) = self.read("proc/meminfo") else {
            return 0;
        };
        meminfo
            .lines()
            .find_map(|line| line.strip_prefix("MemTotal:"))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|kb| kb.parse::<u64>().ok())
            .map(|kb| u32::try_from(kb / 1024).unwrap_or(u32::MAX))
            .unwrap_or(0)
    }
}

impl ProfileSource for SystemSource {
    fn kernel_cmdline(&self) -> Option<String> {
        self.read("proc/cmdline")
    }
    fn edition_env(&self) -> Option<String> {
        self.edition_env.clone()
    }
    fn edition_file(&self) -> Option<String> {
        fs::read_to_string(&self.edition_file).ok()
    }
    fn kernel_device_info(&self) -> Option<String> {
        self.read("proc/ultar/geraet")
    }
    fn chassis_type(&self) -> Option<String> {
        self.read("sys/class/dmi/id/chassis_type")
    }
    fn has_device_tree(&self) -> bool {
        self.root.join("proc/device-tree/compatible").exists()
    }
    fn model(&self) -> Option<String> {
        self.read("proc/device-tree/model")
            .or_else(|| self.read("sys/class/dmi/id/product_name"))
            .map(|model| model.trim_matches(['\0', '\n', ' ']).to_string())
    }
    fn kernel_id(&self) -> Option<String> {
        self.read("proc/sys/kernel/osrelease")
            .map(|release| release.trim().to_string())
    }
    fn capabilities(&self) -> Capabilities {
        let mut capabilities = Capabilities::default();
        capabilities
            .flags
            .set(Capability::Battery, self.has_battery());
        capabilities.memory_mb = self.memory_mb();
        capabilities
    }
}

/// Profile of the running system, detected once and then served from a cache.
///
/// Only the facts readable without a HAL backend are filled in (edition, form factor,
/// battery, memory). The device daemon publishes the full capability set on the bus.
pub fn current_profile() -> &'static DeviceProfile {
    static PROFILE: OnceLock<DeviceProfile> = OnceLock::new();
    PROFILE.get_or_init(|| ultar_profile::detect(&SystemSource::running()))
}

/// Edition of the running system (service preset: phone, desktop or server).
pub fn current_edition() -> Edition {
    current_profile().edition
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_profile::FormFactor;

    fn tree(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ultar-hal-system-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (path, content) in files {
            let file = root.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, content).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn mini_pc_is_desktop() {
        let root = tree(
            "minipc",
            &[
                ("sys/class/dmi/id/chassis_type", "35\n"),
                ("proc/meminfo", "MemTotal:       95000000 kB\n"),
            ],
        );
        let profile = ultar_profile::detect(&SystemSource::at(&root));
        assert_eq!(profile.edition, Edition::Desktop);
        assert_eq!(profile.form_factor, FormFactor::Desktop);
        assert_eq!(profile.capabilities.memory_mb, 92773);
        assert!(!profile.capabilities.has(Capability::Battery));
    }

    #[test]
    fn phone_from_device_tree_and_battery() {
        let root = tree(
            "phone",
            &[
                ("proc/device-tree/compatible", "samsung,a30"),
                ("proc/device-tree/model", "Samsung A30\0"),
                ("sys/class/power_supply/battery/type", "Battery\n"),
                ("proc/sys/kernel/osrelease", "4.4.177\n"),
            ],
        );
        let profile = ultar_profile::detect(&SystemSource::at(&root));
        assert_eq!(profile.edition, Edition::Phone);
        assert!(profile.capabilities.has(Capability::Battery));
        assert!(profile.device_id.contains("Samsung A30") && profile.device_id.contains("4.4.177"));
    }

    #[test]
    fn configuration_beats_detection() {
        let root = tree(
            "cfg",
            &[
                ("sys/class/dmi/id/chassis_type", "35"),
                ("etc/ultar/edition", "# set by build\nserver\n"),
            ],
        );
        assert_eq!(
            ultar_profile::detect(&SystemSource::at(&root)).edition,
            Edition::Server
        );
        let env = SystemSource::at(&root).with_edition_env("phone");
        assert_eq!(ultar_profile::detect(&env).edition, Edition::Phone);
        let root2 = tree(
            "cmd",
            &[
                ("proc/cmdline", "quiet ultar.edition=desktop"),
                ("etc/ultar/edition", "server"),
            ],
        );
        assert_eq!(
            ultar_profile::detect(&SystemSource::at(&root2)).edition,
            Edition::Desktop
        );
    }

    #[test]
    fn ultar_kernel_description_wins_over_chassis() {
        let root = tree(
            "kern",
            &[
                ("proc/ultar/geraet", "art=server\nausgabe=server\n"),
                ("sys/class/dmi/id/chassis_type", "35"),
            ],
        );
        assert_eq!(
            ultar_profile::detect(&SystemSource::at(&root)).edition,
            Edition::Server
        );
    }

    #[test]
    fn empty_system_falls_back_to_phone() {
        let root = tree("empty", &[]);
        let profile = ultar_profile::detect(&SystemSource::at(&root));
        assert_eq!(profile.edition, Edition::Phone);
        assert_eq!(profile.capabilities.memory_mb, 0);
    }
}
