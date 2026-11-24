//! Test rig for the end-to-end tests: a fake system root, a depot built with a
//! throwaway key, key and state directories.
#![allow(dead_code)]

#[path = "../../../../hal/ultar-hal-system/tests/common/kernel_fmt.rs"]
pub mod kernel_fmt;

use kernel_fmt::temp_root;
use std::path::{Path, PathBuf};
use ultar_bus::messages::device_service::{DeviceInfo, DevicePhase, ListReply};
use ultar_bus::messages::devices::{Availability, Capabilities};
use ultar_devd::bus::{MemoryPublisher, Published};
use ultar_devd::daemon::{Daemon, DaemonConfig};
use ultar_driverpkg::depot::write_signed_index;
use ultar_driverpkg::manifest::Manifest;
use ultar_driverpkg::package::{pack, Limits};
use ultar_driverpkg::payload::{build_payload, build_payload_from};
use ultar_driverpkg::{PrivateKey, TrustedKeys};

pub struct Rig {
    pub root: PathBuf,
    pub depot: PathBuf,
    pub keys: PathBuf,
    pub state: PathBuf,
    pub key: PrivateKey,
}

fn repo_drivers() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../drivers")
}

/// Manifest of a stage-A package: activates a driver built into the kernel.
pub fn builtin_manifest(id: &str, version: &str, class: &str, rules: &str) -> String {
    format!(
        "id = \"{id}\"\nversion = \"{version}\"\nclass = \"{class}\"\n\
         kernel_abi = [\"ultar-kern-abi-1\"]\norigin = \"hand\"\nrank = 50\n\
         name_key = \"driver.{id}.name\"\n\n[match]\n{rules}\n\n\
         [content]\nhal_config = \"builtin/{id}.toml\"\n"
    )
}

impl Rig {
    pub fn new(tag: &str) -> Self {
        let base = temp_root(tag);
        let rig = Self {
            root: base.join("root"),
            depot: base.join("depot"),
            keys: base.join("keys"),
            state: base.join("state"),
            key: PrivateKey::generate(),
        };
        for dir in [&rig.root, &rig.depot, &rig.keys] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let key_file = format!("# throwaway test key\n{}\n", rig.key.public().to_hex());
        std::fs::write(rig.keys.join("test.keys"), key_file).unwrap();
        rig
    }

    pub fn trusted(&self) -> TrustedKeys {
        TrustedKeys::from(self.key.public())
    }

    fn add(&self, manifest: &str, payload: &[u8]) -> PathBuf {
        let parsed = Manifest::parse(manifest).unwrap();
        let bytes = pack(manifest.as_bytes(), payload, &self.key, &Limits::default()).unwrap();
        let path = self
            .depot
            .join(format!("{}-{}.utr", parsed.id, parsed.version));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// A stage-A package with the activation record `builtin/<id>.toml`.
    pub fn add_builtin(&self, manifest: &str, kernel_driver: &str) -> PathBuf {
        let id = Manifest::parse(manifest).unwrap().id;
        let record = format!("kernel_driver = \"{kernel_driver}\"\n").into_bytes();
        let payload = build_payload_from(&[(format!("builtin/{id}.toml"), record)]).unwrap();
        self.add(manifest, &payload)
    }

    /// A package whose payload is given file by file.
    pub fn add_with_files(&self, manifest: &str, files: &[(String, Vec<u8>)]) -> PathBuf {
        self.add(manifest, &build_payload_from(files).unwrap())
    }

    /// The six driver packages of the repository (`drivers/manifests` + `drivers/payload`).
    pub fn add_repo_drivers(&self) {
        let dir = repo_drivers();
        for id in [
            "ahci",
            "gen-hda",
            "gen-intel-eth",
            "gen-ixgbe",
            "intel-display",
            "nvidia-display",
            "gen-xhci",
            "nvme",
        ] {
            let manifest = std::fs::read_to_string(dir.join(format!("manifests/{id}.toml")));
            let payload = build_payload(&dir.join("payload").join(id)).unwrap();
            self.add(&manifest.unwrap(), &payload);
        }
    }

    /// The planned drivers (`drivers/manifests/planned`): loadable modules without
    /// activation record, packed with stand-in files.
    pub fn add_planned_drivers(&self) {
        let dir = repo_drivers().join("manifests/planned");
        let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        paths.sort();
        for path in paths {
            let manifest = std::fs::read_to_string(path).unwrap();
            let files: Vec<(String, Vec<u8>)> = Manifest::parse(&manifest)
                .unwrap()
                .content
                .paths()
                .map(|p| (p.to_string(), b"stand-in".to_vec()))
                .collect();
            self.add_with_files(&manifest, &files);
        }
    }

    /// Writes the signed index over everything currently in the depot.
    pub fn write_index(&self) {
        write_signed_index(&self.depot, &self.key, &self.trusted(), &Limits::default()).unwrap();
    }

    /// Repository drivers + planned drivers + signed index.
    pub fn standard_depot(&self) {
        self.add_repo_drivers();
        self.add_planned_drivers();
        self.write_index();
    }

    pub fn config(&self) -> DaemonConfig {
        DaemonConfig {
            root: self.root.clone(),
            depot: self.depot.clone(),
            keys: self.keys.clone(),
            state: self.state.clone(),
            kernel_abi: None,
        }
    }

    /// Starts a daemon that publishes into memory.
    pub fn start(&self) -> (Daemon, MemoryPublisher) {
        let memory = MemoryPublisher::default();
        let daemon = Daemon::start(self.config(), Box::new(memory.clone())).unwrap();
        (daemon, memory)
    }

    pub fn state_file(&self) -> PathBuf {
        self.state.join("state.toml")
    }
}

/// `(address, phase, active driver id, reason key)` of every row.
pub fn summary(reply: &ListReply) -> Vec<(String, DevicePhase, String, String)> {
    reply
        .devices
        .iter()
        .map(|d| {
            (
                d.address.clone(),
                d.phase,
                d.active.as_ref().map(|a| a.id.clone()).unwrap_or_default(),
                d.reason.as_ref().map(|r| r.key.clone()).unwrap_or_default(),
            )
        })
        .collect()
}

pub fn row<'a>(reply: &'a ListReply, address: &str) -> &'a DeviceInfo {
    reply
        .devices
        .iter()
        .find(|d| d.address == address)
        .unwrap_or_else(|| panic!("{address} not listed"))
}

/// `id` → availability of a capability message.
pub fn availability(capabilities: &Capabilities) -> Vec<(&str, Availability)> {
    capabilities
        .capabilities
        .iter()
        .map(|c| (c.id.as_str(), c.availability))
        .collect()
}

/// The last published state per device: `(device, driver, phase)`.
pub fn last_states(
    published: &Published,
) -> std::collections::BTreeMap<String, (String, ultar_bus::messages::devices::DriverPhase)> {
    published
        .states
        .iter()
        .map(|s| (s.device.clone(), (s.driver.clone(), s.phase)))
        .collect()
}

/// Replaces one occurrence of `from` by `to` (same length) inside a file.
pub fn patch_file(path: &Path, from: &[u8], to: &[u8]) {
    assert_eq!(from.len(), to.len());
    let mut bytes = std::fs::read(path).unwrap();
    let at = bytes
        .windows(from.len())
        .position(|window| window == from)
        .unwrap_or_else(|| panic!("pattern not found in {}", path.display()));
    bytes[at..at + from.len()].copy_from_slice(to);
    std::fs::write(path, bytes).unwrap();
}
