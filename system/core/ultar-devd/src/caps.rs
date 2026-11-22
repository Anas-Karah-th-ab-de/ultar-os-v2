//! Capability names: what a driver class provides and what a driverless device would provide.
//!
//! Names follow `ultar_profile::Capability::as_str` where the profile crate defines the
//! capability (`gpu`, `audio_out`, `wifi`, `bluetooth`, `modem`, `camera`, `sensors`, `npu`);
//! classes without a profile capability use their class name (`net`, `storage`, `bus`, …).

use ultar_driverpkg::{DriverClass, HardwareId};

/// The capability a working driver of this class provides.
pub fn class_capability(class: DriverClass) -> &'static str {
    match class {
        DriverClass::Display => "gpu",
        DriverClass::Audio => "audio_out",
        DriverClass::Sensor => "sensors",
        other => other.as_str(),
    }
}

/// The capability a device would provide judging by its bus class code, for reporting
/// hardware that is present but has no working driver. `None` when the class code does
/// not say (unknown digits never match).
pub fn hardware_hint(id: &HardwareId) -> Option<&'static str> {
    match id {
        HardwareId::Pci { class, .. } => {
            let base = (class >> 16) as u8;
            let pair = (class >> 8) as u16;
            match (base, pair) {
                (0x03, _) => Some("gpu"),
                (0x12, _) => Some("npu"),
                (0x01, _) => Some("storage"),
                (_, 0x0280) => Some("wifi"),
                (_, 0x0200) => Some("net"),
                (_, 0x0401 | 0x0403) => Some("audio_out"),
                (_, 0x0d11) => Some("bluetooth"),
                _ => None,
            }
        }
        HardwareId::Usb { class, .. } => match (class >> 16) as u8 {
            0x0e => Some("camera"),
            0xe0 if (class & 0xffff) == 0x0101 => Some("bluetooth"),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pci(class: u32) -> HardwareId {
        HardwareId::Pci {
            vendor: 0x8086,
            device: 1,
            class,
            subsys: None,
        }
    }

    fn usb(class: u32) -> HardwareId {
        HardwareId::Usb {
            vendor: 0x8087,
            product: 1,
            class,
        }
    }

    #[test]
    fn class_names() {
        assert_eq!(class_capability(DriverClass::Display), "gpu");
        assert_eq!(class_capability(DriverClass::Audio), "audio_out");
        assert_eq!(class_capability(DriverClass::Sensor), "sensors");
        assert_eq!(class_capability(DriverClass::Net), "net");
        assert_eq!(class_capability(DriverClass::Wifi), "wifi");
        assert_eq!(class_capability(DriverClass::Storage), "storage");
        assert_eq!(class_capability(DriverClass::Bus), "bus");
    }

    #[test]
    fn hints_from_class_codes() {
        assert_eq!(hardware_hint(&pci(0x03_0000)), Some("gpu"));
        assert_eq!(hardware_hint(&pci(0x12_0000)), Some("npu"));
        assert_eq!(hardware_hint(&pci(0x01_0802)), Some("storage"));
        assert_eq!(hardware_hint(&pci(0x02_8000)), Some("wifi"));
        assert_eq!(hardware_hint(&pci(0x02_0000)), Some("net"));
        assert_eq!(hardware_hint(&pci(0x04_0100)), Some("audio_out"));
        assert_eq!(hardware_hint(&pci(0x04_0300)), Some("audio_out"));
        assert_eq!(hardware_hint(&pci(0x0d_1100)), Some("bluetooth"));
        assert_eq!(hardware_hint(&pci(0x08_0501)), None);
        // Unknown digits (filled with f by the HAL) never produce a hint.
        assert_eq!(hardware_hint(&pci(0x00ff_ffff)), None);
        assert_eq!(hardware_hint(&pci(0x02_ffff)), None);
        assert_eq!(hardware_hint(&usb(0xe0_0101)), Some("bluetooth"));
        assert_eq!(hardware_hint(&usb(0x0e_0000)), Some("camera"));
        assert_eq!(hardware_hint(&usb(0)), None);
        assert_eq!(hardware_hint(&HardwareId::Acpi("PNP0C0A".into())), None);
    }
}
