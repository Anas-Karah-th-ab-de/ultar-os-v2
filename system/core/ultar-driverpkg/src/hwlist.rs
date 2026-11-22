//! The hardware list text format read by `udrv match` and written by inventory tools.
//!
//! ```text
//! # comment
//! machine ThinkPad P1 Gen 3
//! pci 8086:0d4c class=020000 subsys=17aa:22be at=00:1f.6 label=intel-i219-lm
//! usb 8087:0026 class=e00101 at=1-14
//! dt samsung,exynos7885-decon alias=samsung,decon
//! acpi PNP0C0A
//! platform ps2
//! ```
//! Without `at=` the address is derived from the id and its occurrence (`pci:8086:0d4c#0`).

use crate::error::Coded;
use crate::hardware::{DeviceDescriptor, HardwareId};
use crate::pattern::parse_id_pair;
use std::collections::BTreeMap;
use std::fmt;

/// What is wrong with a line of a hardware list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HwListProblem {
    /// The line does not start with `machine`, `pci`, `usb`, `dt`, `acpi` or `platform`.
    UnknownKind(String),
    /// The identifier after the kind is missing.
    MissingId,
    /// The identifier is not valid for its kind.
    BadId(String),
    /// A PCI device has no `class=` attribute.
    MissingClass,
    /// An attribute is unknown for this kind or has an invalid value.
    BadAttribute(String),
    /// Two devices share the same address.
    DuplicateAddress(String),
}

/// A rejected hardware list, with the 1-based line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwListError {
    /// Line number of the offending line.
    pub line: usize,
    /// What is wrong.
    pub problem: HwListProblem,
}

impl Coded for HwListError {
    fn code(&self) -> &'static str {
        match self.problem {
            HwListProblem::UnknownKind(_) => "driverpkg.hwlist.unknown-kind",
            HwListProblem::MissingId => "driverpkg.hwlist.missing-id",
            HwListProblem::BadId(_) => "driverpkg.hwlist.bad-id",
            HwListProblem::MissingClass => "driverpkg.hwlist.missing-class",
            HwListProblem::BadAttribute(_) => "driverpkg.hwlist.bad-attribute",
            HwListProblem::DuplicateAddress(_) => "driverpkg.hwlist.duplicate-address",
        }
    }
}

impl fmt::Display for HwListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: line {}", self.code(), self.line)?;
        match &self.problem {
            HwListProblem::UnknownKind(s)
            | HwListProblem::BadId(s)
            | HwListProblem::BadAttribute(s)
            | HwListProblem::DuplicateAddress(s) => write!(f, ": {s}"),
            HwListProblem::MissingId | HwListProblem::MissingClass => Ok(()),
        }
    }
}

impl std::error::Error for HwListError {}

/// Attributes of one device line.
#[derive(Default)]
struct Attributes {
    class: Option<u32>,
    subsys: Option<(u16, u16)>,
    at: Option<String>,
    label: Option<String>,
    aliases: Vec<String>,
}

fn parse_class(value: &str) -> Option<u32> {
    if value.len() != 6 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(value, 16).ok()
}

fn parse_attributes<'a>(
    kind: &str,
    tokens: impl Iterator<Item = &'a str>,
) -> Result<Attributes, HwListProblem> {
    let mut attrs = Attributes::default();
    for token in tokens {
        let bad = || HwListProblem::BadAttribute(token.to_string());
        let (key, value) = token.split_once('=').ok_or_else(bad)?;
        if value.is_empty() {
            return Err(bad());
        }
        match (key, kind) {
            ("class", "pci" | "usb") => attrs.class = Some(parse_class(value).ok_or_else(bad)?),
            ("subsys", "pci") => attrs.subsys = Some(parse_id_pair(value).ok_or_else(bad)?),
            ("alias", "dt" | "acpi") => attrs.aliases.push(value.to_string()),
            ("at", _) => attrs.at = Some(value.to_string()),
            ("label", _) => attrs.label = Some(value.to_string()),
            _ => return Err(bad()),
        }
    }
    Ok(attrs)
}

fn build_id(kind: &str, id: &str, attrs: &Attributes) -> Result<HardwareId, HwListProblem> {
    let pair = || parse_id_pair(id).ok_or_else(|| HwListProblem::BadId(id.to_string()));
    match kind {
        "pci" => {
            let (vendor, device) = pair()?;
            let class = attrs.class.ok_or(HwListProblem::MissingClass)?;
            Ok(HardwareId::Pci {
                vendor,
                device,
                class,
                subsys: attrs.subsys,
            })
        }
        "usb" => {
            let (vendor, product) = pair()?;
            Ok(HardwareId::Usb {
                vendor,
                product,
                class: attrs.class.unwrap_or(0),
            })
        }
        "dt" => Ok(HardwareId::Dt(id.to_string())),
        "acpi" => Ok(HardwareId::Acpi(id.to_ascii_uppercase())),
        "platform" => Ok(HardwareId::Platform(id.to_string())),
        other => Err(HwListProblem::UnknownKind(other.to_string())),
    }
}

fn alias_id(kind: &str, alias: &str) -> HardwareId {
    if kind == "acpi" {
        HardwareId::Acpi(alias.to_ascii_uppercase())
    } else {
        HardwareId::Dt(alias.to_string())
    }
}

fn parse_device(
    line: &str,
    counts: &mut BTreeMap<String, usize>,
) -> Result<DeviceDescriptor, HwListProblem> {
    let mut tokens = line.split_whitespace();
    let kind = tokens.next().unwrap_or_default();
    if !matches!(kind, "pci" | "usb" | "dt" | "acpi" | "platform") {
        return Err(HwListProblem::UnknownKind(kind.to_string()));
    }
    let id = tokens.next().ok_or(HwListProblem::MissingId)?;
    let attrs = parse_attributes(kind, tokens)?;
    let primary = build_id(kind, id, &attrs)?;
    let address = match &attrs.at {
        Some(at) => format!("{kind}:{at}"),
        None => {
            let count = counts.entry(primary.to_string()).or_insert(0);
            let address = format!("{primary}#{count}");
            *count += 1;
            address
        }
    };
    let mut device = DeviceDescriptor::new(address, primary);
    device.aliases = attrs
        .aliases
        .iter()
        .map(|alias| alias_id(kind, alias))
        .collect();
    device.label = attrs.label;
    Ok(device)
}

/// Parses a hardware list. The `machine` line (anywhere in the file) applies to all devices.
pub fn parse_hardware_list(text: &str) -> Result<Vec<DeviceDescriptor>, HwListError> {
    let mut devices: Vec<DeviceDescriptor> = Vec::new();
    let mut machine = None;
    let mut counts = BTreeMap::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        if let Some(model) = line.strip_prefix("machine ") {
            machine = Some(model.trim().to_string());
            continue;
        }
        let device = parse_device(line, &mut counts).map_err(|problem| HwListError {
            line: index + 1,
            problem,
        })?;
        if devices.iter().any(|d| d.address == device.address) {
            let problem = HwListProblem::DuplicateAddress(device.address);
            return Err(HwListError {
                line: index + 1,
                problem,
            });
        }
        devices.push(device);
    }
    for device in &mut devices {
        device.machine.clone_from(&machine);
    }
    Ok(devices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(text: &str) -> HwListProblem {
        parse_hardware_list(text).unwrap_err().problem
    }

    #[test]
    fn parses_all_kinds() {
        let text = "# inventory\nmachine ThinkPad P1 Gen 3\n\
            pci 8086:0d4c class=020000 subsys=17aa:22be at=00:1f.6 label=intel-i219-lm # wired\n\
            usb 8087:0026 class=e00101\nusb 8087:0026\n\
            dt samsung,exynos7885-decon alias=samsung,decon\nacpi pnp0c0a alias=pnp0c0b\nplatform ps2\n";
        let devices = parse_hardware_list(text).unwrap();
        assert_eq!(devices.len(), 6);
        assert_eq!(devices[0].address, "pci:00:1f.6");
        let expected = HardwareId::Pci {
            vendor: 0x8086,
            device: 0x0d4c,
            class: 0x020000,
            subsys: Some((0x17aa, 0x22be)),
        };
        assert_eq!(devices[0].primary, expected);
        assert_eq!(devices[0].label.as_deref(), Some("intel-i219-lm"));
        assert_eq!(devices[1].address, "usb:8087:0026#0");
        assert_eq!(devices[2].address, "usb:8087:0026#1");
        assert_eq!(
            devices[2].primary,
            HardwareId::Usb {
                vendor: 0x8087,
                product: 0x26,
                class: 0
            }
        );
        assert_eq!(
            devices[3].aliases,
            vec![HardwareId::Dt("samsung,decon".into())]
        );
        assert_eq!(devices[4].primary, HardwareId::Acpi("PNP0C0A".into()));
        assert_eq!(devices[4].aliases, vec![HardwareId::Acpi("PNP0C0B".into())]);
        assert_eq!(devices[5].primary, HardwareId::Platform("ps2".into()));
        assert!(devices
            .iter()
            .all(|d| d.machine.as_deref() == Some("ThinkPad P1 Gen 3")));
        assert!(parse_hardware_list("").unwrap().is_empty());
    }

    #[test]
    fn reports_problems_with_line() {
        assert_eq!(
            problem("serial 1234"),
            HwListProblem::UnknownKind("serial".into())
        );
        assert_eq!(problem("pci"), HwListProblem::MissingId);
        assert_eq!(
            problem("pci 8086 class=020000"),
            HwListProblem::BadId("8086".into())
        );
        assert_eq!(problem("usb 8087:*"), HwListProblem::BadId("8087:*".into()));
        assert_eq!(problem("pci 8086:0d4c"), HwListProblem::MissingClass);
        assert_eq!(
            problem("pci 8086:0d4c class=02"),
            HwListProblem::BadAttribute("class=02".into())
        );
        assert_eq!(
            problem("usb 8087:0026 subsys=1:2"),
            HwListProblem::BadAttribute("subsys=1:2".into())
        );
        assert_eq!(
            problem("platform ps2 loose"),
            HwListProblem::BadAttribute("loose".into())
        );
        assert_eq!(
            problem("platform ps2 at="),
            HwListProblem::BadAttribute("at=".into())
        );
        assert_eq!(
            problem("platform a at=x\nplatform b at=x"),
            HwListProblem::DuplicateAddress("platform:x".into())
        );
        let err = parse_hardware_list("\n\nbogus line").unwrap_err();
        assert_eq!(err.line, 3);
        assert_eq!(
            err.to_string(),
            "driverpkg.hwlist.unknown-kind: line 3: bogus"
        );
        assert_eq!(
            parse_hardware_list("pci").unwrap_err().to_string(),
            "driverpkg.hwlist.missing-id: line 1"
        );
    }
}
