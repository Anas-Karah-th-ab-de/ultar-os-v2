//! Test support: produces the files the Ultar kernel exposes, with formats copied from the
//! kernel source (`kernel/ultar-kern/src`: `devices/sysfs.rs`, `geraet.rs`, `procfs.rs`).
//! Also builds Linux-style sysfs trees.
//!
//! Shared by the tests of `ultar-hal-system` and `ultar-devd`.
#![allow(dead_code)]

#[path = "sysfs_tree.rs"]
mod sysfs_tree;

use std::collections::BTreeMap;
use std::path::Path;
use ultar_driverpkg::hwlist::parse_hardware_list;
use ultar_driverpkg::HardwareId;

/// A PCI function as the kernel's `pci::PciDev` describes it.
#[derive(Debug, Clone)]
pub struct KDev {
    pub slot: String,
    pub vendor: u16,
    pub device: u16,
    pub class: u32,
    pub subsys: (u16, u16),
    pub label: Option<String>,
}

impl KDev {
    pub fn new(slot: &str, vendor: u16, device: u16, class: u32) -> Self {
        Self {
            slot: slot.to_string(),
            vendor,
            device,
            class,
            subsys: (0, 0),
            label: None,
        }
    }

    fn parts(&self) -> (u8, u8, u8) {
        (
            (self.class >> 16) as u8,
            (self.class >> 8) as u8,
            self.class as u8,
        )
    }

    /// `pci.rs::PciDev::id`.
    pub fn id(&self) -> String {
        format!("{} {:04x}:{:04x}", self.slot, self.vendor, self.device)
    }

    pub fn address(&self) -> String {
        format!("pci:{}", self.slot)
    }
}

/// A USB device as the xHCI driver announces it.
#[derive(Debug, Clone)]
pub struct KUsb {
    pub port: String,
    pub vendor: u16,
    pub product: u16,
    pub class: u32,
    pub label: Option<String>,
}

/// A machine: model, PCI functions and USB devices.
#[derive(Debug, Clone, Default)]
pub struct Machine {
    pub vendor: String,
    pub model: String,
    pub pci: Vec<KDev>,
    pub usb: Vec<KUsb>,
    /// Probe errors by slot (`ultar_probe` then says `failed: <driver> <error>`).
    pub probe_errors: BTreeMap<String, String>,
}

const INTEL_ETH: &[u16] = &[
    0x100e, 0x100f, 0x1004, 0x1008, 0x1010, 0x1015, 0x1019, 0x101e, 0x1026, 0x1075, 0x1076, 0x1079,
    0x107c, 0x105e, 0x105f, 0x10a4, 0x10b9, 0x10d3, 0x10f6, 0x150c, 0x153a, 0x153b, 0x155a, 0x1559,
    0x15a0, 0x15a1, 0x15a2, 0x15a3, 0x156f, 0x1570, 0x15b7, 0x15b8, 0x15b9, 0x15bb, 0x15bc, 0x15bd,
    0x15be, 0x15d6, 0x15d7, 0x15d8, 0x15e3, 0x0d4e, 0x0d4f, 0x0d4c, 0x0d4d, 0x15fb, 0x15fc, 0x1a1e,
    0x1a1f,
];
const E610: &[u16] = &[0x57ae, 0x57af, 0x57b0, 0x57b1, 0x57b2];

/// `drivers::ALL` in order with each driver's `matches` (virtio by its fixed ids):
/// name and origin of the first driver that takes the device.
pub fn kernel_driver(d: &KDev) -> Option<(&'static str, &'static str)> {
    let (c, s, p) = d.parts();
    let virtio = |ids: [u16; 2]| d.vendor == 0x1af4 && ids.contains(&d.device);
    if virtio([0x1001, 0x1042]) {
        Some(("virtio-blk", "hand"))
    } else if virtio([0x1000, 0x1041]) {
        Some(("virtio-net", "hand"))
    } else if (c, s, p) == (0x01, 0x08, 0x02) {
        Some(("nvme", "hand"))
    } else if virtio([0x1005, 0x1044]) {
        Some(("virtio-rng", "hand"))
    } else if c == 0x06 {
        Some(("bruecke", "hand"))
    } else if (c, s, p) == (0x01, 0x06, 0x01) {
        Some(("gen-ahci", "werkstatt"))
    } else if c == 0x03 && matches!((d.vendor, d.device), (0x1234, 0x1111) | (0x80ee, 0xbeef)) {
        Some(("gen-bochs-vga", "werkstatt"))
    } else if (c, s) == (0x04, 0x03) {
        Some(("gen-hda", "werkstatt"))
    } else if d.vendor == 0x8086 && c == 0x02 && INTEL_ETH.contains(&d.device) {
        Some(("gen-intel-eth", "werkstatt"))
    } else if d.vendor == 0x8086 && c == 0x02 && E610.contains(&d.device) {
        Some(("gen-ixgbe", "werkstatt"))
    } else if d.vendor == 0x8086 && (c, s) == (0x0c, 0x05) {
        Some(("gen-smbus", "werkstatt"))
    } else if (c, s, p) == (0x0c, 0x03, 0x30) {
        Some(("gen-xhci", "werkstatt"))
    } else {
        None
    }
}

impl Machine {
    /// Builds a machine from a hardware list of `ultar-driverpkg` (`*.hw`). Devices without
    /// `at=` get consecutive slots `00:NN.0` / ports.
    pub fn from_hwlist(vendor: &str, text: &str) -> Self {
        let devices = parse_hardware_list(text).unwrap();
        let mut machine = Machine {
            vendor: vendor.to_string(),
            model: devices[0].machine.clone().unwrap_or_default(),
            ..Machine::default()
        };
        for device in devices {
            let given = device.address.split_once(':').map(|(_, at)| at.to_string());
            let given = given.filter(|at| !at.contains('#'));
            match device.primary {
                HardwareId::Pci {
                    vendor,
                    device: id,
                    class,
                    subsys,
                } => {
                    let slot = given.unwrap_or_else(|| format!("00:{:02x}.0", machine.pci.len()));
                    machine.pci.push(KDev {
                        slot,
                        vendor,
                        device: id,
                        class,
                        subsys: subsys.unwrap_or((0, 0)),
                        label: device.label,
                    });
                }
                HardwareId::Usb {
                    vendor,
                    product,
                    class,
                } => machine.usb.push(KUsb {
                    port: given.unwrap_or_else(|| format!("{}", machine.usb.len() + 1)),
                    vendor,
                    product,
                    class,
                    label: device.label,
                }),
                _ => {}
            }
        }
        machine
    }

    /// Slot → `Ok(driver, origin)`, `Err(error)` or `None` as `driver.rs::bind_all` decides.
    pub fn outcome(&self, d: &KDev) -> Option<Result<(&'static str, &'static str), String>> {
        let found = kernel_driver(d)?;
        Some(match self.probe_errors.get(&d.slot) {
            Some(error) => Err(error.clone()),
            None => Ok(found),
        })
    }

    /// `/proc/ultar/geraet`: `geraet.rs::text` + `grafik.rs::text`.
    pub fn device_info(&self, kind: &str, chassis: &str) -> String {
        let mut s = format!(
            "art={kind}\nausgabe=desktop\nquelle=smbios\ngehaeuse={chassis}\nacpi_profil=2\nakku=nein\nhersteller={}\nmodell={}\nanzeige=gop 1920x1080\n",
            self.vendor, self.model
        );
        let cards = self.pci.iter().filter(|d| d.parts().0 == 0x03);
        for (i, d) in cards.enumerate() {
            let path = match (self.outcome(d), d.vendor) {
                (Some(Ok((n, _))), _) => format!("Treiber {n}"),
                (_, 0x8086) => {
                    "Firmware-Bildspeicher; eigener Treiber: Bauplan intel-xe-anzeige".to_string()
                }
                (_, 0x10de) => {
                    "Firmware-Bildspeicher; NVIDIA: keine offene Anzeige-Norm".to_string()
                }
                (_, 0x1002) => "Firmware-Bildspeicher; AMD: Bauplan amd-dcn-anzeige".to_string(),
                _ => "Firmware-Bildspeicher".to_string(),
            };
            s += &format!("grafik{i}={:04x}:{:04x} {path}\n", d.vendor, d.device);
        }
        s
    }

    /// Writes the files of an Ultar kernel below `root`: `/proc/version`, the device
    /// description and the sysfs device tree of `devices/sysfs.rs` (with the `ultar_*`
    /// files, the driver register and the bind/unbind control files).
    pub fn write_ultar_root(&self, root: &Path) {
        // the tree describes the machine as it is now: nothing of an earlier one stays
        let _ = std::fs::remove_dir_all(root.join("sys"));
        write(root, "proc/version", ULTAR_VERSION);
        write(
            root,
            "proc/ultar/geraet",
            &self.device_info("pc", "35 (Mini-PC)"),
        );
        write(root, "sys/kernel/ultar/driver_abi", "1\n");
        self.write_ultar_sysfs(root);
    }
}

/// `procfs.rs`, file `version`.
pub const ULTAR_VERSION: &str =
    "Linux version 6.8.0-ultar-kern (Ultar-Kern 0.4, KM Code Craft) #1\n";

/// Writes a file below `root`, creating directories.
pub fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A fresh, empty directory below the system temp directory.
pub fn temp_root(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("ultar-root-{}-{n}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

pub fn beelink() -> Machine {
    Machine::from_hwlist(
        "AZW",
        include_str!("../../../../core/ultar-driverpkg/tests/fixtures/beelink-gti15.hw"),
    )
}

pub fn thinkpad() -> Machine {
    Machine::from_hwlist(
        "LENOVO",
        include_str!("../../../../core/ultar-driverpkg/tests/fixtures/thinkpad-p1-gen3.hw"),
    )
}
