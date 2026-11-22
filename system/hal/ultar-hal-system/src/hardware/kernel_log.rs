//! Parser for the kernel log of the Ultar kernel (`/proc/ultar/protokoll`).
//!
//! The Ultar kernel (0.4) has no dedicated device or driver file below `/proc/ultar`.
//! What user space can read about devices is what the kernel printed into its log ring
//! (64 KiB, `klog.rs`). The lines understood here, with the kernel code that formats them:
//!
//! | Line | Source |
//! |---|---|
//! | `[pci] 12 Gerät(e) gefunden` | `main.rs` after `pci::scan()` |
//! | `[treiber] 00:1f.6 8086:0d4c -> gen-intel-eth (werkstatt) ok` | `driver.rs::bind_all` |
//! | `[treiber] 00:1f.6 8086:0d4c -> gen-intel-eth FEHLER: <text>` | `driver.rs::bind_all` |
//! | `  00:1f.6 8086:0d4c  Ethernet   gen-intel-eth` | self test table / console `geraete` |
//! | `WERKSTATT {"pci":"00:02.0","hersteller":"8086",…}` | `werkstatt.rs::report` |
//! | `gen-xhci  werkstatt  2 Gerät(e)` | console `treiber` |
//! | `[xhci] Controller 8086:777d: …` / `[xhci] Port 3: 8087:0026` | `drivers/gen_xhci.rs` |
//!
//! Only `[treiber]` lines are printed at every boot. The device table and the `WERKSTATT`
//! lines exist only after a self-test boot or after the matching console command.

use super::class_names::{class_from_name, PartialClass};

/// Size of the kernel's log ring; a log of this size has wrapped and lost its beginning.
pub const KERNEL_LOG_RING: usize = 64 * 1024;

/// Kernel output token "Datenträger" (a USB disk announced by the xHCI driver); a token
/// of the kernel's text output, not interface text.
const DISK_TOKEN: &str = "Datentr\u{e4}ger";
/// Kernel output token "Gerät(e)" that ends a row of the driver register.
const DEVICES_TOKEN: &str = "Ger\u{e4}t(e)";

/// Result of the kernel's driver search for one PCI function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogBinding {
    /// A built-in driver took the device.
    Bound {
        /// Kernel driver name.
        driver: String,
        /// `hand` or `werkstatt` when the line states it.
        origin: Option<String>,
    },
    /// A built-in driver matched but its probe failed.
    Failed {
        /// Kernel driver name; `None` when only the device table reported the failure.
        driver: Option<String>,
        /// The kernel's error text (technical, German).
        error: String,
    },
    /// No built-in driver matched.
    Unbound,
}

/// One PCI function as far as the log describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogPciDevice {
    /// `bb:dd.f` in lower-case hex.
    pub slot: String,
    /// Vendor id.
    pub vendor: u16,
    /// Device id.
    pub device: u16,
    /// Class code as far as known.
    pub class: PartialClass,
    /// Subsystem vendor and device (only from `WERKSTATT` lines).
    pub subsys: Option<(u16, u16)>,
    /// Outcome of the kernel's driver search; `None` when no line states it.
    pub binding: Option<LogBinding>,
}

/// One USB device announced by the xHCI driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogUsbDevice {
    /// Index of the controller in start order.
    pub controller: u32,
    /// Port path below the controller (`3`, `2.1`).
    pub port: String,
    /// Vendor id.
    pub vendor: u16,
    /// Product id.
    pub product: u16,
    /// Class, subclass, protocol when the line implies them, else `0`.
    pub class: u32,
}

/// One row of the console command `treiber` (the built-in driver register).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogDriverRow {
    /// Kernel driver name.
    pub name: String,
    /// `hand` or `werkstatt`.
    pub origin: String,
    /// Number of devices the driver took.
    pub devices: u32,
}

/// Everything the log says about devices and drivers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KernelLog {
    /// Device count of the PCI scan, when its line is still in the ring.
    pub pci_count: Option<usize>,
    /// PCI functions in order of first appearance.
    pub pci: Vec<LogPciDevice>,
    /// USB devices in order of first appearance.
    pub usb: Vec<LogUsbDevice>,
    /// Rows of the driver register, when the console command was run.
    pub drivers: Vec<LogDriverRow>,
    /// `true` when a full device table (self test or `geraete`) was found.
    pub has_device_table: bool,
    /// `true` when the ring has wrapped: early lines may be missing.
    pub wrapped: bool,
}

fn hex16(text: &str) -> Option<u16> {
    (text.len() == 4 && text.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| u16::from_str_radix(text, 16).ok())
        .flatten()
}

fn id_pair(text: &str) -> Option<(u16, u16)> {
    let (vendor, device) = text.split_once(':')?;
    Some((hex16(vendor)?, hex16(device)?))
}

/// `bb:dd.f` with two hex digits each and a function digit 0–7.
fn slot(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let shape = bytes.len() == 7
        && bytes[2] == b':'
        && bytes[5] == b'.'
        && [0, 1, 3, 4].iter().all(|&i| bytes[i].is_ascii_hexdigit())
        && (b'0'..=b'7').contains(&bytes[6]);
    shape.then(|| text.to_ascii_lowercase())
}

/// Splits `"<slot> <vendor>:<device><rest>"`.
fn device_head(line: &str) -> Option<(String, (u16, u16), &str)> {
    let slot = slot(line.get(..7)?)?;
    let rest = line.get(7..)?.strip_prefix(' ')?;
    let ids = id_pair(rest.get(..9)?)?;
    Some((slot, ids, rest.get(9..)?))
}

/// The value of a string field of a `WERKSTATT` line (the kernel writes no escapes
/// in the fields read here).
fn json_field<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("\"{key}\":\"");
    let start = json.find(&marker)? + marker.len();
    let end = json[start..].find('"')?;
    Some(&json[start..start + end])
}

impl KernelLog {
    /// Parses the text of `/proc/ultar/protokoll`. Unknown lines are ignored.
    pub fn parse(text: &str) -> Self {
        let mut log = Self {
            wrapped: text.len() >= KERNEL_LOG_RING,
            ..Self::default()
        };
        let mut controller: Option<u32> = None;
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("[treiber] ") {
                log.bind_line(rest);
            } else if let Some(rest) = trimmed.strip_prefix("[pci] ") {
                log.pci_count = rest
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse().ok())
                    .or(log.pci_count);
            } else if let Some(json) = trimmed.strip_prefix("WERKSTATT ") {
                log.blueprint_line(json);
            } else if let Some(rest) = trimmed.strip_prefix("[xhci] ") {
                log.xhci_line(rest, &mut controller);
            } else if let Some(error) = trimmed.strip_prefix("Fehler: ") {
                log.table_error(error);
            } else if !log.table_row(trimmed) {
                log.register_row(trimmed);
            }
        }
        log
    }

    fn entry(&mut self, slot: String, ids: (u16, u16)) -> &mut LogPciDevice {
        let position = self.pci.iter().position(|d| d.slot == slot);
        let index = match position {
            Some(index) => index,
            None => {
                self.pci.push(LogPciDevice {
                    slot,
                    vendor: ids.0,
                    device: ids.1,
                    class: PartialClass::UNKNOWN,
                    subsys: None,
                    binding: None,
                });
                self.pci.len() - 1
            }
        };
        let device = &mut self.pci[index];
        if (device.vendor, device.device) != ids {
            // Another device at this slot after a later boot message: start afresh.
            device.vendor = ids.0;
            device.device = ids.1;
            device.class = PartialClass::UNKNOWN;
            device.subsys = None;
            device.binding = None;
        }
        device
    }

    /// `<slot> <ids> -> <driver> (<origin>) ok` or `… -> <driver> FEHLER: <text>`.
    fn bind_line(&mut self, rest: &str) {
        let Some((slot, ids, tail)) = device_head(rest) else {
            return;
        };
        let Some(tail) = tail.strip_prefix(" -> ") else {
            return;
        };
        let binding = if let Some((driver, error)) = tail.split_once(" FEHLER: ") {
            LogBinding::Failed {
                driver: Some(driver.trim().to_string()),
                error: error.trim().to_string(),
            }
        } else if let Some(head) = tail.strip_suffix(" ok") {
            let (driver, origin) = match head.rsplit_once(" (") {
                Some((driver, origin)) => (driver, origin.strip_suffix(')')),
                None => (head, None),
            };
            LogBinding::Bound {
                driver: driver.trim().to_string(),
                origin: origin.map(str::to_string),
            }
        } else {
            return;
        };
        self.entry(slot, ids).binding = Some(binding);
    }

    /// A row of the device table: `<slot> <ids>  <class name>  <driver|FEHLER|->`.
    fn table_row(&mut self, line: &str) -> bool {
        let Some((slot, ids, tail)) = device_head(line) else {
            return false;
        };
        let tail = tail.trim();
        let Some((name, state)) = tail.rsplit_once(' ') else {
            return false;
        };
        let class = class_from_name(name.trim());
        self.has_device_table = true;
        let device = self.entry(slot, ids);
        if class.known_digits > device.class.known_digits {
            device.class = class;
        }
        match state {
            "-" => device.binding = Some(LogBinding::Unbound),
            "FEHLER" => {
                if !matches!(device.binding, Some(LogBinding::Failed { .. })) {
                    device.binding = Some(LogBinding::Failed {
                        driver: None,
                        error: String::new(),
                    });
                }
            }
            driver => {
                if !matches!(&device.binding, Some(LogBinding::Bound { driver: d, .. }) if d == driver)
                {
                    device.binding = Some(LogBinding::Bound {
                        driver: driver.to_string(),
                        origin: None,
                    });
                }
            }
        }
        true
    }

    /// `      Fehler: <text>` below a table row with state `FEHLER`.
    fn table_error(&mut self, error: &str) {
        if let Some(LogBinding::Failed { error: slot, .. }) =
            self.pci.last_mut().and_then(|d| d.binding.as_mut())
        {
            if slot.is_empty() {
                *slot = error.trim().to_string();
            }
        }
    }

    fn blueprint_line(&mut self, json: &str) {
        let parsed = (|| {
            let slot = slot(json_field(json, "pci")?)?;
            let vendor = hex16(json_field(json, "hersteller")?)?;
            let device = hex16(json_field(json, "geraet")?)?;
            let class = json_field(json, "klasse")?;
            let class = (class.len() == 6)
                .then(|| u32::from_str_radix(class, 16).ok())
                .flatten()?;
            let subsys = json_field(json, "subsystem").and_then(id_pair);
            Some((slot, (vendor, device), class, subsys))
        })();
        let Some((slot, ids, class, subsys)) = parsed else {
            return;
        };
        let device = self.entry(slot, ids);
        device.class = PartialClass::exact(class);
        device.subsys = subsys.filter(|pair| *pair != (0, 0));
        if device.binding.is_none() {
            device.binding = Some(LogBinding::Unbound);
        }
    }

    fn xhci_line(&mut self, rest: &str, controller: &mut Option<u32>) {
        if rest.starts_with("Controller ") {
            *controller = Some(controller.map_or(0, |index| index.saturating_add(1)));
            return;
        }
        let Some((port, what)) = rest
            .strip_prefix("Port ")
            .and_then(|tail| tail.split_once(": "))
        else {
            return;
        };
        let port_ok = !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit() || b == b'.');
        let ids = what
            .split_whitespace()
            .find_map(|word| id_pair(word.trim_matches(['(', ')'])));
        let (Some((vendor, product)), true) = (ids, port_ok) else {
            return;
        };
        let class = if what.starts_with("Tastatur") {
            0x03_0101
        } else if what.starts_with(DISK_TOKEN) {
            0x08_0650
        } else if what.starts_with("Hub") {
            0x09_0000
        } else {
            0
        };
        let controller = controller.unwrap_or(0);
        if let Some(known) = self
            .usb
            .iter_mut()
            .find(|d| d.controller == controller && d.port == port)
        {
            known.vendor = vendor;
            known.product = product;
            known.class = class.max(known.class);
        } else {
            self.usb.push(LogUsbDevice {
                controller,
                port: port.to_string(),
                vendor,
                product,
                class,
            });
        }
    }

    /// `<name> <origin> <n> Gerät(e)` printed by the console command `treiber`.
    fn register_row(&mut self, line: &str) {
        let words: Vec<&str> = line.split_whitespace().collect();
        let [name, origin, count, unit] = words.as_slice() else {
            return;
        };
        let Ok(devices) = count.parse::<u32>() else {
            return;
        };
        if *unit != DEVICES_TOKEN || name.starts_with('[') {
            return;
        }
        let row = LogDriverRow {
            name: (*name).to_string(),
            origin: (*origin).to_string(),
            devices,
        };
        match self.drivers.iter_mut().find(|d| d.name == row.name) {
            Some(known) => *known = row,
            None => self.drivers.push(row),
        }
    }
}
