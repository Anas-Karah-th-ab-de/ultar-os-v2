//! Hardware identifiers and device descriptions supplied by the HAL or the installer.

use std::fmt;

/// One identifier under which a device can be matched to a driver.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HardwareId {
    /// A PCI function.
    Pci {
        /// Vendor id.
        vendor: u16,
        /// Device id.
        device: u16,
        /// 24-bit class code (class, subclass, programming interface).
        class: u32,
        /// Subsystem vendor and device id, if known.
        subsys: Option<(u16, u16)>,
    },
    /// A USB device.
    Usb {
        /// Vendor id.
        vendor: u16,
        /// Product id.
        product: u16,
        /// 24-bit class, subclass, protocol; `0` when the class is defined per interface.
        class: u32,
    },
    /// A device-tree `compatible` string.
    Dt(String),
    /// An ACPI hardware id (`_HID` / `_CID`).
    Acpi(String),
    /// A named platform device without bus id.
    Platform(String),
}

impl fmt::Display for HardwareId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pci { vendor, device, .. } => write!(f, "pci:{vendor:04x}:{device:04x}"),
            Self::Usb {
                vendor, product, ..
            } => write!(f, "usb:{vendor:04x}:{product:04x}"),
            Self::Dt(compatible) => write!(f, "dt:{compatible}"),
            Self::Acpi(hid) => write!(f, "acpi:{hid}"),
            Self::Platform(name) => write!(f, "platform:{name}"),
        }
    }
}

/// A device found on the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceDescriptor {
    /// Unique position of the device on this machine (e.g. `pci:00:1f.6`).
    pub address: String,
    /// The most specific identifier.
    pub primary: HardwareId,
    /// Further identifiers (additional device-tree compatibles, ACPI `_CID`s …).
    pub aliases: Vec<HardwareId>,
    /// Optional technical label from the inventory (data, not translated text).
    pub label: Option<String>,
    /// Machine model (DMI product or device-tree model) for model-specific drivers.
    pub machine: Option<String>,
}

impl DeviceDescriptor {
    /// Creates a descriptor with a single identifier.
    pub fn new(address: impl Into<String>, primary: HardwareId) -> Self {
        Self {
            address: address.into(),
            primary,
            aliases: Vec::new(),
            label: None,
            machine: None,
        }
    }

    /// Adds a further identifier.
    pub fn with_alias(mut self, id: HardwareId) -> Self {
        self.aliases.push(id);
        self
    }

    /// Sets the inventory label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the machine model.
    pub fn with_machine(mut self, machine: impl Into<String>) -> Self {
        self.machine = Some(machine.into());
        self
    }

    /// All identifiers, the primary one first.
    pub fn ids(&self) -> impl Iterator<Item = &HardwareId> {
        std::iter::once(&self.primary).chain(self.aliases.iter())
    }

    /// Stable key for remembering decisions about this device across boots:
    /// address plus primary identifier, so different hardware at the same address is a new device.
    pub fn identity(&self) -> String {
        format!("{}={}", self.address, self.primary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_ids() {
        let pci = HardwareId::Pci {
            vendor: 0x8086,
            device: 0x0d4c,
            class: 0x020000,
            subsys: None,
        };
        assert_eq!(pci.to_string(), "pci:8086:0d4c");
        let usb = HardwareId::Usb {
            vendor: 0x8087,
            product: 0x26,
            class: 0xe00101,
        };
        assert_eq!(usb.to_string(), "usb:8087:0026");
        assert_eq!(HardwareId::Dt("a,b".into()).to_string(), "dt:a,b");
        assert_eq!(
            HardwareId::Acpi("PNP0C0A".into()).to_string(),
            "acpi:PNP0C0A"
        );
        assert_eq!(
            HardwareId::Platform("ps2".into()).to_string(),
            "platform:ps2"
        );
    }

    #[test]
    fn builds_descriptors() {
        let device =
            DeviceDescriptor::new("dt:soc/display", HardwareId::Dt("samsung,decon".into()))
                .with_alias(HardwareId::Dt("samsung,exynos7-decon".into()))
                .with_label("panel")
                .with_machine("Samsung A30");
        assert_eq!(device.ids().count(), 2);
        assert_eq!(device.identity(), "dt:soc/display=dt:samsung,decon");
        assert_eq!(device.label.as_deref(), Some("panel"));
        assert_eq!(device.machine.as_deref(), Some("Samsung A30"));
    }
}
