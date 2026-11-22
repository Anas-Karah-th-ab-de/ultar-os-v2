//! Test support: produces the texts the Ultar kernel exposes, with format strings copied
//! from the kernel source (`kernel/ultar-kern/src`, version 0.4). Each function names the
//! kernel function it mirrors. Also builds Linux-style sysfs trees.
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
    /// Probe errors by slot (the kernel then prints `FEHLER`).
    pub probe_errors: BTreeMap<String, String>,
}

/// `pci.rs::class_name`.
pub fn class_name(c: u8, s: u8, p: u8) -> &'static str {
    match (c, s, p) {
        (0x01, 0x01, _) => "IDE-Controller",
        (0x01, 0x06, 0x01) => "SATA (AHCI)",
        (0x01, 0x08, 0x02) => "NVMe-Speicher",
        (0x01, 0x00, _) => "SCSI-Speicher",
        (0x01, _, _) => "Massenspeicher",
        (0x02, 0x00, _) => "Ethernet",
        (0x02, 0x80, _) => "Netzwerk (WLAN u. a.)",
        (0x02, _, _) => "Netzwerk",
        (0x03, 0x00, _) => "Grafik (VGA)",
        (0x03, _, _) => "Grafik",
        (0x04, 0x03, _) => "Audio (HD Audio)",
        (0x04, _, _) => "Multimedia",
        (0x06, 0x00, _) => "Host-Brücke",
        (0x06, 0x01, _) => "ISA-Brücke",
        (0x06, 0x04, _) => "PCI-Brücke",
        (0x06, _, _) => "Brücke",
        (0x07, _, _) => "Kommunikation (seriell)",
        (0x08, _, _) => "Systemperipherie",
        (0x09, _, _) => "Eingabe",
        (0x0c, 0x03, 0x30) => "USB 3 (xHCI)",
        (0x0c, 0x03, 0x20) => "USB 2 (EHCI)",
        (0x0c, 0x03, _) => "USB (alt)",
        (0x0c, 0x05, _) => "SMBus",
        (0x0c, _, _) => "Serieller Bus",
        (0x0d, _, _) => "Funk (Bluetooth u. a.)",
        (0x10, _, _) => "Verschlüsselung",
        (0x11, _, _) => "Signalverarbeitung",
        (0x12, _, _) => "Rechenbeschleuniger (NPU)",
        _ => "unbekannt",
    }
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

/// `werkstatt.rs::norm` – only the blueprint kind.
fn blueprint_kind(d: &KDev) -> &'static str {
    let (c, s, p) = d.parts();
    match (c, s, p) {
        (0x01, 0x06, 0x01) => "ahci",
        (0x01, 0x08, 0x02) => "nvme",
        (0x0c, 0x03, 0x30) => "xhci",
        (0x0c, 0x03, 0x20) => "ehci",
        (0x04, 0x03, _) => "hda",
        (0x03, _, _) if d.vendor == 0x8086 => "intel-xe-anzeige",
        (0x03, _, _) if d.vendor == 0x1002 => "amd-dcn-anzeige",
        (0x03, 0x00, 0x00) => "vga-fb",
        (0x02, 0x00, _) if d.vendor == 0x8086 => "intel-eth",
        (0x02, 0x00, _) if d.vendor == 0x10ec => "realtek-eth",
        (0x02, 0x80, _) => "wlan",
        (0x0c, 0x05, _) => "smbus",
        (0x06, _, _) => "bruecke",
        _ if d.vendor == 0x1af4 => "virtio",
        _ => "unbekannt",
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

    /// The boot part of the log: `main.rs` (scan line), `driver.rs::bind_all`, xHCI ports.
    pub fn boot_log(&self) -> String {
        let mut s = format!("[pci] {} Gerät(e) gefunden\n", self.pci.len());
        for d in &self.pci {
            match (kernel_driver(d), self.outcome(d)) {
                (Some((name, origin)), Some(Ok(_))) => {
                    s += &format!("[treiber] {} -> {} ({}) ok\n", d.id(), name, origin);
                }
                (Some((name, _)), Some(Err(e))) => {
                    s += &format!("[treiber] {} -> {} FEHLER: {e}\n", d.id(), name);
                }
                _ => {}
            }
        }
        let controller = self
            .pci
            .iter()
            .find(|d| matches!(self.outcome(d), Some(Ok(("gen-xhci", _)))));
        if let (Some(d), false) = (controller, self.usb.is_empty()) {
            s += &format!(
                "[xhci] Controller {:04x}:{:04x}: {} Port(s), {} Slots, Kontext {} Byte\n",
                d.vendor, d.device, 16, 64, 32
            );
            for u in &self.usb {
                s += &format!(
                    "[xhci] Port {}: {:04x}:{:04x}\n",
                    u.port, u.vendor, u.product
                );
            }
        }
        s
    }

    fn state(&self, d: &KDev) -> &'static str {
        match self.outcome(d) {
            Some(Ok((name, _))) => name,
            Some(Err(_)) => "FEHLER",
            None => "-",
        }
    }

    /// The device table of `selftest.rs::run`.
    pub fn selftest_table(&self) -> String {
        let mut s = String::from("[test] PCI-Geräte:\n");
        for d in &self.pci {
            let (c, sub, p) = d.parts();
            s += &format!(
                "  {}  {:<28} {}\n",
                d.id(),
                class_name(c, sub, p),
                self.state(d)
            );
            if let Some(Err(e)) = self.outcome(d) {
                s += &format!("      Fehler: {e}\n");
            }
        }
        s + "[test] Datenträger (nur lesen):\n"
    }

    /// Console command `geraete` (`shell.rs`).
    pub fn console_devices(&self) -> String {
        let mut s = String::new();
        for d in &self.pci {
            let (c, sub, p) = d.parts();
            s += &format!(
                "{}  {:<26} {}\n",
                d.id(),
                class_name(c, sub, p),
                self.state(d)
            );
        }
        s
    }

    /// Console command `treiber` (`shell.rs`): the built-in driver register.
    pub fn console_drivers(&self) -> String {
        const ALL: &[(&str, &str)] = &[
            ("virtio-blk", "hand"),
            ("virtio-net", "hand"),
            ("nvme", "hand"),
            ("virtio-rng", "hand"),
            ("bruecke", "hand"),
            ("gen-ahci", "werkstatt"),
            ("gen-bochs-vga", "werkstatt"),
            ("gen-hda", "werkstatt"),
            ("gen-intel-eth", "werkstatt"),
            ("gen-ixgbe", "werkstatt"),
            ("gen-smbus", "werkstatt"),
            ("gen-xhci", "werkstatt"),
        ];
        let mut s = String::new();
        for (name, origin) in ALL {
            let n = self
                .pci
                .iter()
                .filter(|d| matches!(self.outcome(d), Some(Ok((bound, _))) if bound == *name))
                .count();
            s += &format!("{:<14} {:<10} {} Gerät(e)\n", name, origin, n);
        }
        s + &format!("{:<14} {:<10} Tastatur (Abfrage)\n", "ps2kbd", "hand")
    }

    /// `werkstatt.rs::report`: blueprints of the devices without a working driver.
    pub fn workshop_report(&self) -> String {
        let mut lines = Vec::new();
        for d in &self.pci {
            let error = match self.outcome(d) {
                Some(Ok(_)) => continue,
                Some(Err(e)) => format!("\"{e}\""),
                None => "null".to_string(),
            };
            let kind = blueprint_kind(d);
            if kind == "bruecke" {
                continue;
            }
            let (c, sub, p) = d.parts();
            lines.push(format!(
                "{{\"pci\":\"{}\",\"hersteller\":\"{:04x}\",\"geraet\":\"{:04x}\",\"subsystem\":\"{:04x}:{:04x}\",\"klasse\":\"{:02x}{:02x}{:02x}\",\"rev\":{},\"art\":\"{}\",\"bauplan\":\"{kind}\",\"norm\":\"{}\",\"bars\":[{}],\"faehigkeiten\":[{}],\"fehler\":{}}}",
                d.slot, d.vendor, d.device, d.subsys.0, d.subsys.1, c, sub, p, 1,
                class_name(c, sub, p), "keine offene Norm bekannt – Registerbeschreibung/Datenblatt nötig",
                "{\"bar\":0,\"adresse\":\"0xfe000000\",\"groesse\":16384,\"io\":false}", "1,5,16", error
            ));
        }
        if lines.is_empty() {
            return "[werkstatt] alle Geräte haben einen Treiber\n".to_string();
        }
        let mut s = format!(
            "[werkstatt] {} Gerät(e) ohne Treiber – Baupläne:\n",
            lines.len()
        );
        for line in lines {
            s += &format!("WERKSTATT {line}\n");
        }
        s
    }

    /// The log after a self-test boot (`main.rs`: self test, then workshop report).
    pub fn selftest_log(&self) -> String {
        self.boot_log() + &self.selftest_table() + &self.workshop_report()
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

    /// Writes the files of an Ultar kernel below `root` (`/proc/version`, log, description).
    pub fn write_ultar_root(&self, root: &Path, log: &str) {
        write(root, "proc/version", ULTAR_VERSION);
        write(root, "proc/ultar/protokoll", log);
        write(
            root,
            "proc/ultar/geraet",
            &self.device_info("pc", "35 (Mini-PC)"),
        );
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
