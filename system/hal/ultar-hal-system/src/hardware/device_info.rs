//! Parser for the device description of the Ultar kernel (`/proc/ultar/geraet`).
//!
//! The file is `key=value` text written by `geraet.rs::text()` followed by the lines of
//! `grafik.rs::text()`:
//!
//! ```text
//! art=laptop
//! hersteller=LENOVO
//! modell=ThinkPad P1 Gen 3
//! anzeige=gop 3840x2160
//! grafik0=8086:9bc4 Firmware-Bildspeicher; eigener Treiber: Bauplan intel-xe-anzeige
//! grafik1=1234:1111 Treiber gen-bochs-vga
//! ```

/// The display path that currently shows the picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveDisplay {
    /// Name of the display path (`gop`, `gen-bochs-vga`, …); `-` when the kernel knows none.
    pub name: String,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// One graphics card and what drives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsCard {
    /// PCI vendor id.
    pub vendor: u16,
    /// PCI device id.
    pub device: u16,
    /// The kernel driver operating the card; `None` when it only shows the firmware picture.
    pub driver: Option<String>,
}

/// The parts of `/proc/ultar/geraet` that matter for devices and drivers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KernelDeviceInfo {
    /// Machine vendor from SMBIOS (`hersteller`).
    pub vendor: Option<String>,
    /// Machine model from SMBIOS (`modell`).
    pub model: Option<String>,
    /// The active display, `None` for `anzeige=keine` or a missing line.
    pub display: Option<ActiveDisplay>,
    /// Graphics cards in kernel order.
    pub graphics: Vec<GraphicsCard>,
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn parse_display(value: &str) -> Option<ActiveDisplay> {
    let (name, size) = value.trim().rsplit_once(' ')?;
    let (width, height) = size.split_once('x')?;
    Some(ActiveDisplay {
        name: name.trim().to_string(),
        width: width.parse().ok()?,
        height: height.parse().ok()?,
    })
}

fn parse_card(value: &str) -> Option<GraphicsCard> {
    let (ids, path) = value.trim().split_once(' ').unwrap_or((value.trim(), ""));
    let (vendor, device) = ids.split_once(':')?;
    Some(GraphicsCard {
        vendor: u16::from_str_radix(vendor, 16).ok()?,
        device: u16::from_str_radix(device, 16).ok()?,
        driver: path.strip_prefix("Treiber ").and_then(non_empty),
    })
}

impl KernelDeviceInfo {
    /// Parses the file's text. Unknown keys and malformed lines are ignored.
    pub fn parse(text: &str) -> Self {
        let mut info = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "hersteller" => info.vendor = non_empty(value),
                "modell" => info.model = non_empty(value),
                "anzeige" => info.display = parse_display(value),
                key if key.starts_with("grafik") => info.graphics.extend(parse_card(value)),
                _ => {}
            }
        }
        info
    }

    /// The kernel driver operating the card with these ids, if any.
    pub fn graphics_driver(&self, vendor: u16, device: u16) -> Option<&str> {
        self.graphics
            .iter()
            .find(|card| card.vendor == vendor && card.device == device)
            .and_then(|card| card.driver.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Built from `geraet.rs::text()` + `grafik.rs::text()` for the ThinkPad.
    const THINKPAD: &str = "art=laptop\nausgabe=desktop\nquelle=smbios\ngehaeuse=10 (Notebook)\n\
        acpi_profil=2\nakku=ja\nhersteller=LENOVO\nmodell=ThinkPad P1 Gen 3\nanzeige=gop 3840x2160\n\
        grafik0=8086:9bc4 Firmware-Bildspeicher; eigener Treiber: Bauplan intel-xe-anzeige\n\
        grafik1=10de:1eb6 Firmware-Bildspeicher; NVIDIA: keine offene Anzeige-Norm\n";

    #[test]
    fn parses_machine_and_graphics() {
        let info = KernelDeviceInfo::parse(THINKPAD);
        assert_eq!(info.vendor.as_deref(), Some("LENOVO"));
        assert_eq!(info.model.as_deref(), Some("ThinkPad P1 Gen 3"));
        assert_eq!(
            info.display,
            Some(ActiveDisplay {
                name: "gop".into(),
                width: 3840,
                height: 2160
            })
        );
        assert_eq!(info.graphics.len(), 2);
        assert_eq!(info.graphics_driver(0x8086, 0x9bc4), None);
        assert_eq!(info.graphics_driver(0x1234, 0x1111), None);
    }

    #[test]
    fn card_with_driver_and_edge_cases() {
        let info = KernelDeviceInfo::parse(
            "anzeige=gen-bochs-vga 1920x1080\ngrafik0=1234:1111 Treiber gen-bochs-vga\n\
             grafik1=zzzz:1111 Treiber x\ngrafik2=1af4:1050\nmodell=\nnoise\n",
        );
        assert_eq!(info.graphics_driver(0x1234, 0x1111), Some("gen-bochs-vga"));
        assert_eq!(info.graphics.len(), 2);
        assert_eq!(info.graphics[1].driver, None);
        assert_eq!(info.model, None);
        assert_eq!(info.display.map(|d| d.name), Some("gen-bochs-vga".into()));
        // The early default of the kernel (before detection) and a machine without picture.
        let early = KernelDeviceInfo::parse("art=pc\nausgabe=desktop\nquelle=standard\n");
        assert_eq!(early, KernelDeviceInfo::default());
        assert_eq!(KernelDeviceInfo::parse("anzeige=keine\n").display, None);
    }
}
