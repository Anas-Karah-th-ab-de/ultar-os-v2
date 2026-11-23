//! `/sys/bus` and `/sys/kernel`: the device list, the driver register and their control files in
//! the Linux sysfs layout (plus `ultar_*` files where Linux has nothing). See the module above.

use super::{platform, usb};
use crate::driver::DEVICES;
use crate::fs::{DirEnt, Node};
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

const EACCES: i64 = 13;
const ENODEV: i64 = 19;
const EINVAL: i64 = 22;
const EOPNOTSUPP: i64 = 95;

const PCI_DEV_FILES: [&str; 13] = [
    "class", "device", "enable", "revision", "subsystem_device", "subsystem_vendor", "uevent", "ultar_drivers", "ultar_health", "ultar_probe", "ultar_state", "ultar_unbind", "vendor",
];
const PCI_DRV_FILES: [&str; 6] = ["bind", "uevent", "ultar_enabled", "ultar_origin", "ultar_unbind", "unbind"];
const USB_DEV_FILES: [&str; 9] = ["bDeviceClass", "bDeviceProtocol", "bDeviceSubClass", "bcdDevice", "busnum", "devnum", "idProduct", "idVendor", "uevent"];
const USB_IF_FILES: [&str; 5] = ["bInterfaceClass", "bInterfaceNumber", "bInterfaceProtocol", "bInterfaceSubClass", "uevent"];
const KERNEL_ULTAR: [&str; 4] = ["devices", "driver_abi", "drivers", "uevents"];

/// Whether the host path belongs to this tree.
pub fn is_tree(path: &str) -> bool {
    ["/sys/bus", "/sys/kernel", "/sys/class"].iter().any(|r| path == *r || path.strip_prefix(r).is_some_and(|x| x.starts_with('/')))
}

/// Entries this tree adds to the listing of `/sys`.
pub const TOP: [&str; 3] = ["bus", "class", "kernel"];

fn file(s: String) -> Option<Node> {
    Some(Node::File(Arc::new(s.into_bytes())))
}

fn dir(names: impl IntoIterator<Item = String>, dirs: bool) -> Option<Node> {
    Some(Node::Dir(names.into_iter().map(|name| DirEnt { name, dir: dirs, size: 0 }).collect()))
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|n| String::from(*n)).collect()
}

fn pci_names() -> Vec<String> {
    DEVICES.lock().iter().map(|b| super::pci_name(&b.dev)).collect()
}

fn pci_index(name: &str) -> Option<usize> {
    DEVICES.lock().iter().position(|b| super::pci_name(&b.dev) == name)
}

/// USB devices and their interfaces (`1-2`, `1-2:1.0`).
fn usb_entries() -> Vec<String> {
    let mut v = Vec::new();
    for d in usb::list() {
        for i in &d.ifaces {
            v.push(format!("{}:1.{}", d.name(), i.number));
        }
        v.push(d.name());
    }
    v.sort();
    v
}

/// A USB device by name and, for an interface name, the interface.
fn usb_find(name: &str) -> Option<(usb::UsbDev, Option<usb::Iface>)> {
    let (dev, iface) = match name.split_once(':') {
        Some((d, i)) => (d, Some(i.strip_prefix("1.")?.parse::<u8>().ok()?)),
        None => (name, None),
    };
    let d = usb::list().into_iter().find(|d| d.name() == dev)?;
    match iface {
        None => Some((d, None)),
        Some(n) => {
            let i = d.ifaces.iter().find(|i| i.number == n)?.clone();
            Some((d, Some(i)))
        }
    }
}

/// Node at a host path of the tree.
pub fn open(path: &str) -> Option<Node> {
    let rest = path.strip_prefix("/sys/")?;
    let p: Vec<&str> = rest.split('/').filter(|c| !c.is_empty()).collect();
    match p.as_slice() {
        ["bus"] => dir(names(&["pci", "platform", "usb"]), true),
        ["bus", "pci"] => Some(Node::Dir(alloc::vec![
            DirEnt { name: String::from("devices"), dir: true, size: 0 },
            DirEnt { name: String::from("drivers"), dir: true, size: 0 },
            DirEnt { name: String::from("drivers_probe"), dir: false, size: 0 },
        ])),
        ["bus", "usb" | "platform"] => dir(names(&["devices", "drivers"]), true),
        ["bus", "pci", "drivers_probe"] => file(String::new()),
        ["bus", "pci", "devices"] => dir(pci_names(), true),
        ["bus", "usb", "devices"] => dir(usb_entries(), true),
        ["bus", "platform", "devices"] => dir(platform::list().iter().map(|d| String::from(d.name)), true),
        ["bus", "pci", "drivers"] => dir(crate::drivers::ALL.iter().map(|d| String::from(d.name())), true),
        ["bus", "usb", "drivers"] => dir(names(&usb::DRIVERS), true),
        ["bus", "platform", "drivers"] => dir(names(&platform::DRIVERS), true),
        ["bus", "pci", "devices", n, rest @ ..] => pci_device(n, rest),
        ["bus", "pci", "drivers", d, rest @ ..] => pci_driver(d, rest),
        ["bus", "usb", "devices", n, rest @ ..] => usb_device(n, rest),
        ["bus", bus @ ("usb" | "platform"), "drivers", d, rest @ ..] => function_driver(bus, d, rest),
        ["bus", "platform", "devices", n, rest @ ..] => platform_device(n, rest),
        ["class"] => dir(names(&["input"]), true),
        ["class", "input"] => dir(crate::input::all().into_iter().map(|n| format!("event{n}")), true),
        ["class", "input", n, rest @ ..] => input_class(n, rest),
        ["kernel"] => Some(Node::Dir(alloc::vec![
            DirEnt { name: String::from("uevent_seqnum"), dir: false, size: 0 },
            DirEnt { name: String::from("ultar"), dir: true, size: 0 },
        ])),
        ["kernel", "uevent_seqnum"] => file(format!("{}\n", super::events::seqnum())),
        ["kernel", "ultar"] => dir(names(&KERNEL_ULTAR), false),
        ["kernel", "ultar", "driver_abi"] => file(format!("{}\n", super::DRIVER_ABI)),
        ["kernel", "ultar", "devices"] => file(device_table()),
        ["kernel", "ultar", "drivers"] => file(driver_table()),
        ["kernel", "ultar", "uevents"] => file(format!("lost {}\n{}", super::events::lost(), super::events::recent())),
        _ => None,
    }
}

/// `/sys/class/input/eventN` and below.
fn input_class(n: &str, rest: &[&str]) -> Option<Node> {
    let index = crate::input::parse(n).filter(|&i| crate::input::exists(i))?;
    let entry = |name: &str, dir: bool| DirEnt { name: String::from(name), dir, size: 0 };
    match rest {
        [] => Some(Node::Dir(alloc::vec![entry("dev", false), entry("device", true), entry("uevent", false)])),
        ["device"] => Some(Node::Dir(alloc::vec![entry("name", false), entry("uevent", false)])),
        _ => file(crate::input::sys_file(index, rest)?),
    }
}

/// The right a write to control file `path` needs: announcing an input node again (`uevent`) is
/// file administration (the seat service); everything else is driver work.
pub fn right_for(path: &str) -> crate::isolation::rights::Rights {
    if path.starts_with("/sys/class/") {
        crate::isolation::rights::Rights::FILE_ADMIN
    } else {
        crate::isolation::rights::Rights::LOAD_DRIVER
    }
}

/// Target of a symbolic link of the tree (`driver` of a device, a device in a driver's directory).
pub fn link(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/sys/bus/")?;
    let p: Vec<&str> = rest.split('/').collect();
    match p.as_slice() {
        ["pci", "devices", n, "driver"] => {
            let i = pci_index(n)?;
            DEVICES.lock()[i].driver.map(|d| format!("../../drivers/{d}"))
        }
        ["pci", "drivers", d, n] if bound_to(n) == Some(*d) => Some(format!("../../devices/{n}")),
        ["usb", "devices", n, "driver"] => usb_find(n)?.1?.driver.map(|d| format!("../../drivers/{d}")),
        ["usb", "drivers", d, n] if usb_find(n).and_then(|x| x.1).and_then(|i| i.driver) == Some(*d) => Some(format!("../../devices/{n}")),
        ["platform", "devices", n, "driver"] => platform::get(n).map(|d| format!("../../drivers/{}", d.driver)),
        ["platform", "drivers", d, n] if platform::get(n).is_some_and(|x| x.driver == *d) => Some(format!("../../devices/{n}")),
        _ => None,
    }
}

fn bound_to(name: &str) -> Option<&'static str> {
    let i = pci_index(name)?;
    DEVICES.lock()[i].driver
}

fn pci_device(n: &str, rest: &[&str]) -> Option<Node> {
    let i = pci_index(n)?;
    let (b, drv, disabled, state, error, slots) = {
        let devs = DEVICES.lock();
        let b = &devs[i];
        let error = b.error.as_ref().map(|e| format!("{} {}", b.tried.unwrap_or("-"), e.replace('\n', " ")));
        (b.dev.clone(), b.driver, b.disabled, super::state(b), error, b.slots.clone())
    };
    let class = (b.class as u32) << 16 | (b.subclass as u32) << 8 | b.prog_if as u32;
    let text = match rest {
        [] => {
            let mut v = names(&PCI_DEV_FILES);
            if drv.is_some() {
                v.push(String::from("driver"));
            }
            return dir(v, false);
        }
        ["vendor"] => format!("0x{:04x}\n", b.vendor),
        ["device"] => format!("0x{:04x}\n", b.device),
        ["class"] => format!("0x{class:06x}\n"),
        ["revision"] => format!("0x{:02x}\n", b.rev),
        ["subsystem_vendor"] => format!("0x{:04x}\n", b.subsys_vendor),
        ["subsystem_device"] => format!("0x{:04x}\n", b.subsys),
        ["enable"] => format!("{}\n", u8::from(!disabled)),
        ["ultar_state"] => format!("{state}\n"),
        ["ultar_probe"] => match (drv, error) {
            (Some(_), _) => String::from("ok\n"),
            (None, Some(e)) => format!("failed: {e}\n"),
            (None, None) => String::from("none\n"),
        },
        ["ultar_health"] => super::health(i)?,
        ["ultar_drivers"] => crate::drivers::ALL.iter().filter(|d| d.matches(&b)).map(|d| format!("{}\n", d.name())).collect(),
        ["ultar_unbind"] => match drv.and_then(super::driver) {
            None => String::from("unbound\n"),
            Some(d) => unbind_word(super::unbind_refusal(d, &slots)),
        },
        ["uevent"] => pci_uevent(&b, drv, class),
        _ => return None,
    };
    file(text)
}

fn unbind_word(refusal: Option<&str>) -> String {
    match refusal {
        None => String::from("supported\n"),
        Some(why) => format!("unsupported: {why}\n"),
    }
}

fn pci_uevent(b: &crate::pci::PciDev, drv: Option<&str>, class: u32) -> String {
    let mut s = String::new();
    if let Some(d) = drv {
        s += &format!("DRIVER={d}\n");
    }
    s += &format!(
        "PCI_CLASS={class:X}\nPCI_ID={:04X}:{:04X}\nPCI_SUBSYS_ID={:04X}:{:04X}\nPCI_SLOT_NAME={}\nMODALIAS=pci:v{:08X}d{:08X}sv{:08X}sd{:08X}bc{:02X}sc{:02X}i{:02X}\n",
        b.vendor, b.device, b.subsys_vendor, b.subsys, super::pci_name(b), b.vendor, b.device, b.subsys_vendor, b.subsys, b.class, b.subclass, b.prog_if
    );
    s
}

fn pci_driver(d: &str, rest: &[&str]) -> Option<Node> {
    let drv = super::driver(d)?;
    let mine: Vec<String> = DEVICES.lock().iter().filter(|b| b.driver == Some(drv.name())).map(|b| super::pci_name(&b.dev)).collect();
    let text = match rest {
        [] => {
            let mut v = names(&PCI_DRV_FILES);
            v.extend(mine);
            return dir(v, false);
        }
        ["bind" | "unbind"] => String::new(),
        ["uevent"] => format!("DRIVER={d}\n"),
        ["ultar_origin"] => format!("{}\n", drv.origin()),
        ["ultar_enabled"] => format!("{}\n", u8::from(super::driver_on(d))),
        ["ultar_unbind"] => unbind_word(drv.can_remove().err()),
        _ => return None,
    };
    file(text)
}

fn usb_device(n: &str, rest: &[&str]) -> Option<Node> {
    let (d, iface) = usb_find(n)?;
    let text = match (iface, rest) {
        (None, []) => return dir(names(&USB_DEV_FILES), false),
        (Some(i), []) => {
            let mut v = names(&USB_IF_FILES);
            if i.driver.is_some() {
                v.push(String::from("driver"));
            }
            return dir(v, false);
        }
        (None, ["idVendor"]) => format!("{:04x}\n", d.vendor),
        (None, ["idProduct"]) => format!("{:04x}\n", d.product),
        (None, ["bcdDevice"]) => format!("{:04x}\n", d.bcd),
        (None, ["bDeviceClass"]) => format!("{:02x}\n", d.class[0]),
        (None, ["bDeviceSubClass"]) => format!("{:02x}\n", d.class[1]),
        (None, ["bDeviceProtocol"]) => format!("{:02x}\n", d.class[2]),
        (None, ["busnum"]) => format!("{}\n", d.hc + 1),
        (None, ["devnum"]) => format!("{}\n", d.slot),
        (None, ["uevent"]) => format!(
            "DEVTYPE=usb_device\nPRODUCT={:x}/{:x}/{:x}\nTYPE={}/{}/{}\nBUSNUM={:03}\nDEVNUM={:03}\n",
            d.vendor, d.product, d.bcd, d.class[0], d.class[1], d.class[2], d.hc + 1, d.slot
        ),
        (Some(i), ["bInterfaceNumber"]) => format!("{:02x}\n", i.number),
        (Some(i), ["bInterfaceClass"]) => format!("{:02x}\n", i.class),
        (Some(i), ["bInterfaceSubClass"]) => format!("{:02x}\n", i.sub),
        (Some(i), ["bInterfaceProtocol"]) => format!("{:02x}\n", i.proto),
        (Some(i), ["uevent"]) => format!(
            "DEVTYPE=usb_interface\n{}PRODUCT={:x}/{:x}/{:x}\nINTERFACE={}/{}/{}\n",
            i.driver.map(|x| format!("DRIVER={x}\n")).unwrap_or_default(),
            d.vendor, d.product, d.bcd, i.class, i.sub, i.proto
        ),
        _ => return None,
    };
    file(text)
}

/// A driver of the USB or platform bus: part of another driver or of the kernel core.
fn function_driver(bus: &str, d: &str, rest: &[&str]) -> Option<Node> {
    let (known, mine, why): (bool, Vec<String>, &str) = if bus == "usb" {
        let mine = usb_entries().into_iter().filter(|n| usb_find(n).and_then(|x| x.1).and_then(|i| i.driver) == Some(d)).collect();
        (usb::DRIVERS.contains(&d), mine, "part of the xhci driver")
    } else {
        let mine = platform::list().iter().filter(|x| x.driver == d).map(|x| String::from(x.name)).collect();
        (platform::DRIVERS.contains(&d), mine, "part of the kernel core")
    };
    if !known {
        return None;
    }
    let text = match rest {
        [] => {
            let mut v = names(&["uevent", "ultar_origin", "ultar_unbind"]);
            v.extend(mine);
            return dir(v, false);
        }
        ["uevent"] => format!("DRIVER={d}\n"),
        ["ultar_origin"] => String::from("builtin\n"),
        ["ultar_unbind"] => format!("unsupported: {why}\n"),
        _ => return None,
    };
    file(text)
}

fn platform_device(n: &str, rest: &[&str]) -> Option<Node> {
    let d = platform::get(n)?;
    match rest {
        [] => dir(names(&["driver", "modalias", "uevent", "ultar_state"]), false),
        ["uevent"] => file(platform::uevent(&d)),
        ["modalias"] => file(format!("platform:{}\n", d.driver)),
        ["ultar_state"] => file(String::from("bound\n")),
        _ => None,
    }
}

/// `/sys/kernel/ultar/devices`: `bus name ids class subsystem driver state probe`, `-` for none.
fn device_table() -> String {
    let mut s = String::new();
    for b in DEVICES.lock().iter() {
        let d = &b.dev;
        let probe = if b.driver.is_some() { "ok" } else if b.error.is_some() { "failed" } else { "none" };
        s += &format!(
            "pci {} {:04x}:{:04x} {:02x}{:02x}{:02x} {:04x}:{:04x} {} {} {probe}\n",
            super::pci_name(d), d.vendor, d.device, d.class, d.subclass, d.prog_if, d.subsys_vendor, d.subsys, b.driver.unwrap_or("-"), super::state(b)
        );
    }
    for d in usb::list() {
        let drv = d.ifaces.iter().find_map(|i| i.driver).unwrap_or("-");
        let state = if drv == "-" { "unbound" } else { "bound" };
        s += &format!("usb {} {:04x}:{:04x} {:02x}{:02x}{:02x} - {drv} {state} -\n", d.name(), d.vendor, d.product, d.class[0], d.class[1], d.class[2]);
    }
    for d in platform::list() {
        s += &format!("platform {} - - - {} bound -\n", d.name, d.driver);
    }
    s
}

/// `/sys/kernel/ultar/drivers`: `name bus origin enabled unbind devices` (devices comma separated).
fn driver_table() -> String {
    let mut s = String::new();
    for drv in crate::drivers::ALL {
        let mine: Vec<String> = DEVICES.lock().iter().filter(|b| b.driver == Some(drv.name())).map(|b| super::pci_name(&b.dev)).collect();
        let unbind = if drv.can_remove().is_ok() { "supported" } else { "unsupported" };
        let devs = if mine.is_empty() { String::from("-") } else { mine.join(",") };
        s += &format!("{} pci {} {} {unbind} {devs}\n", drv.name(), drv.origin(), u8::from(super::driver_on(drv.name())));
    }
    for d in usb::DRIVERS {
        s += &format!("{d} usb builtin 1 unsupported -\n");
    }
    for d in platform::DRIVERS {
        s += &format!("{d} platform builtin 1 unsupported -\n");
    }
    s
}

/// Whether the host path is a file of this tree that takes writes.
pub fn is_control(path: &str) -> bool {
    if let Some(rest) = path.strip_prefix("/sys/class/input/") {
        return matches!(rest.split_once('/'), Some((n, "uevent")) if crate::input::parse(n).is_some());
    }
    let Some(rest) = path.strip_prefix("/sys/bus/pci/") else { return false };
    let p: Vec<&str> = rest.split('/').collect();
    matches!(p.as_slice(), ["drivers_probe"] | ["drivers", _, "bind" | "unbind" | "ultar_enabled"] | ["devices", _, "enable"])
}

/// A write to a control file (the caller checked the right `LOAD_DRIVER`).
pub fn write(path: &str, text: &str) -> Result<(), i64> {
    if let Some(rest) = path.strip_prefix("/sys/class/input/") {
        let index = rest.split('/').next().and_then(crate::input::parse).filter(|&i| crate::input::exists(i)).ok_or(-ENODEV)?;
        return match text {
            "add" | "change" | "remove" => {
                crate::input::uevent(index, text);
                Ok(())
            }
            _ => Err(-EINVAL),
        };
    }
    let Some(rest) = path.strip_prefix("/sys/bus/pci/") else { return Err(-EACCES) };
    let p: Vec<&str> = rest.split('/').collect();
    match p.as_slice() {
        ["drivers_probe"] => super::probe_any(text),
        ["drivers", d, "bind"] => super::bind(d, text),
        ["drivers", d, "unbind"] => match bound_to(text) {
            Some(b) if b == *d => super::unbind(text),
            _ => Err(-ENODEV),
        },
        ["drivers", d, "ultar_enabled"] => super::set_driver_enabled(d, super::flag(text)?),
        ["devices", n, "enable"] => super::set_device_enabled(n, super::flag(text)?),
        ["drivers", ..] | ["devices", ..] => Err(-EINVAL),
        _ => Err(-EOPNOTSUPP),
    }
}
