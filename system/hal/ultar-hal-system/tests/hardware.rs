//! Hardware inventory from Ultar kernel texts and from a sysfs tree.
//!
//! The kernel texts are produced by `common/kernel_fmt.rs`, which copies the kernel's
//! format strings; `fixtures/protokoll-qemu-geraete.txt` holds the device table printed by
//! the kernel in QEMU (docs/legacy/TREIBER.md) together with the matching boot lines.

#[path = "common/kernel_fmt.rs"]
mod kernel_fmt;

use kernel_fmt::{beelink, temp_root, thinkpad, write, KDev, Machine};
use ultar_driverpkg::{DeviceDescriptor, HardwareId};
use ultar_hal_system::hardware::class_names::PartialClass;
use ultar_hal_system::hardware::kernel_log::{KernelLog, LogBinding, KERNEL_LOG_RING};
use ultar_hal_system::{HardwareError, HardwareReader, InventoryNote};

const QEMU: &str = include_str!("fixtures/protokoll-qemu-geraete.txt");

fn pci(device: &DeviceDescriptor) -> (u16, u16, u32, Option<(u16, u16)>) {
    match device.primary {
        HardwareId::Pci {
            vendor,
            device,
            class,
            subsys,
        } => (vendor, device, class, subsys),
        _ => panic!("not PCI: {device:?}"),
    }
}

fn find<'a>(devices: &'a [DeviceDescriptor], address: &str) -> &'a DeviceDescriptor {
    devices
        .iter()
        .find(|d| d.address == address)
        .unwrap_or_else(|| panic!("{address} missing"))
}

#[test]
fn qemu_log_with_console_table() {
    let log = KernelLog::parse(QEMU);
    assert_eq!(log.pci_count, Some(10));
    assert_eq!(log.pci.len(), 10);
    assert!(log.has_device_table && !log.wrapped);
    let nvme = &log.pci[1];
    assert_eq!(
        (nvme.slot.as_str(), nvme.vendor, nvme.device),
        ("00:01.0", 0x1b36, 0x0010)
    );
    assert_eq!(nvme.class, PartialClass::exact(0x01_0802));
    assert_eq!(
        nvme.binding,
        Some(LogBinding::Bound {
            driver: "nvme".into(),
            origin: Some("hand".into())
        })
    );
    let rng = log.pci.iter().find(|d| d.slot == "00:06.0").unwrap();
    assert_eq!(rng.class, PartialClass::UNKNOWN);
    // USB: keyboard, hub (announced twice, kept once), disk behind the hub; the failed port is skipped.
    let usb: Vec<_> = log
        .usb
        .iter()
        .map(|u| (u.port.as_str(), u.vendor, u.product, u.class))
        .collect();
    assert_eq!(
        usb,
        [
            ("5", 0x0627, 0x0001, 0x03_0101),
            ("6", 0x0409, 0x55aa, 0x09_0000),
            ("6.1", 0x46f4, 0x0001, 0x08_0650),
        ]
    );
    assert!(log.drivers.is_empty());
}

#[test]
fn normal_boot_shows_only_bound_devices_without_class() {
    let machine = beelink();
    let root = temp_root("beelink-boot");
    machine.write_ultar_root(&root, &machine.boot_log());
    let inventory = HardwareReader::at(&root).inventory().unwrap();
    assert_eq!(inventory.machine.as_deref(), Some("Beelink GTi15 Ultra"));
    assert!(inventory.sources.kernel_log && !inventory.sources.sysfs_pci);
    // 7 of 11 PCI functions have a built-in driver; 3 USB devices come from the xHCI lines.
    let addresses: Vec<&str> = inventory
        .devices
        .iter()
        .map(|d| d.address.as_str())
        .collect();
    assert_eq!(
        addresses,
        [
            "pci:00:0d.0",
            "pci:00:14.0",
            "pci:00:1f.3",
            "pci:01:00.0",
            "pci:02:00.0",
            "pci:ad:00.0",
            "pci:ad:00.1",
            "usb:0-1",
            "usb:0-2",
            "usb:0-3"
        ]
    );
    let nic = find(&inventory.devices, "pci:ad:00.0");
    assert_eq!(pci(nic), (0x8086, 0x57b0, 0x00ff_ffff, None));
    assert_eq!(nic.machine.as_deref(), Some("Beelink GTi15 Ultra"));
    assert_eq!(
        inventory.notes,
        [
            InventoryNote::BoundDevicesOnly,
            InventoryNote::ClassIncomplete(7)
        ]
    );
}

#[test]
fn selftest_boot_gives_all_devices_and_classes() {
    let machine = beelink();
    let root = temp_root("beelink-selftest");
    machine.write_ultar_root(&root, &machine.selftest_log());
    let inventory = HardwareReader::at(&root).inventory().unwrap();
    assert_eq!(inventory.devices.len(), 14);
    // Driverless devices carry the exact class from the blueprint line …
    assert_eq!(
        pci(find(&inventory.devices, "pci:00:02.0")),
        (0x8086, 0x7d51, 0x03_0000, None)
    );
    assert_eq!(pci(find(&inventory.devices, "pci:ac:00.0")).2, 0x02_8000);
    assert_eq!(pci(find(&inventory.devices, "pci:00:0b.0")).2, 0x12_0000);
    // … bound ones the prefix the class name determines.
    assert_eq!(pci(find(&inventory.devices, "pci:01:00.0")).2, 0x01_0802);
    assert_eq!(pci(find(&inventory.devices, "pci:ad:00.1")).2, 0x02_00ff);
    assert_eq!(pci(find(&inventory.devices, "pci:00:1f.3")).2, 0x04_03ff);
    assert_eq!(inventory.notes, [InventoryNote::ClassIncomplete(3)]);
}

#[test]
fn probe_failure_and_subsystem_from_blueprint() {
    let mut machine = Machine {
        vendor: "QEMU".into(),
        model: "Standard PC (Q35 + ICH9, 2009)".into(),
        ..Machine::default()
    };
    let mut nvme = KDev::new("00:01.0", 0x1b36, 0x0010, 0x01_0802);
    nvme.subsys = (0x1af4, 0x1100);
    machine.pci.push(nvme);
    machine
        .pci
        .push(KDev::new("00:02.0", 0x10ec, 0x8168, 0x02_0000));
    machine
        .probe_errors
        .insert("00:01.0".into(), "BAR0 fehlt".into());
    let log = KernelLog::parse(&machine.selftest_log());
    assert_eq!(
        log.pci[0].binding,
        Some(LogBinding::Failed {
            driver: Some("nvme".into()),
            error: "BAR0 fehlt".into()
        })
    );
    assert_eq!(log.pci[0].subsys, Some((0x1af4, 0x1100)));
    assert_eq!(log.pci[1].binding, Some(LogBinding::Unbound));
    assert_eq!(log.pci[1].class, PartialClass::exact(0x02_0000));
    assert_eq!(log.pci[1].subsys, None);
    // The table alone (console `geraete` without the boot lines) still reports the failure.
    let table = KernelLog::parse(&machine.selftest_table());
    assert_eq!(
        table.pci[0].binding,
        Some(LogBinding::Failed {
            driver: None,
            error: "BAR0 fehlt".into()
        })
    );
}

#[test]
fn console_commands_and_register() {
    let machine = thinkpad();
    let log = KernelLog::parse(&(machine.console_devices() + &machine.console_drivers()));
    assert!(log.has_device_table);
    assert_eq!(log.pci.len(), 23);
    assert_eq!(log.pci_count, None);
    let names: Vec<&str> = log.drivers.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names.len(), 12);
    assert!(names.contains(&"bruecke") && !names.contains(&"ps2kbd"));
    let xhci = log.drivers.iter().find(|d| d.name == "gen-xhci").unwrap();
    assert_eq!((xhci.origin.as_str(), xhci.devices), ("werkstatt", 3));
}

#[test]
fn wrapped_ring_and_garbage_are_survived() {
    let machine = beelink();
    let mut text = "x".repeat(KERNEL_LOG_RING);
    text.replace_range(..40, "0.0 8086:57b0 -> gen-ixgbe (werkstatt) ok\n");
    let boot = machine.boot_log();
    text.replace_range(KERNEL_LOG_RING - boot.len().., &boot);
    assert_eq!(text.len(), KERNEL_LOG_RING);
    let log = KernelLog::parse(&text);
    assert!(log.wrapped);
    assert_eq!(log.pci.len(), 7);
    let root = temp_root("wrapped");
    write(&root, "proc/ultar/protokoll", &text);
    let notes = HardwareReader::at(&root).inventory().unwrap().notes;
    assert_eq!(notes[0], InventoryNote::KernelLogWrapped);
    for garbage in [
        "",
        "[treiber] ",
        "[treiber] zz:00.0 8086:57b0 -> x (hand) ok",
        "[treiber] 00:00.0 8086:57b0 x",
        "[treiber] 00:00.9 8086:57b0 -> x (hand) ok",
        "WERKSTATT {\"pci\":\"00:02.0\",\"hersteller\":\"80\"}",
        "WERKSTATT not json",
        "[xhci] Port x: 8087:0026",
        "[xhci] Port 3: ",
        "[pci] viele Gerät(e) gefunden",
        "a b c Gerät(e)",
        "\u{fffd}\u{fffd} 00:1f.6 8086",
        "      Fehler: lonely",
    ] {
        let log = KernelLog::parse(garbage);
        assert!(
            log.pci.is_empty() && log.usb.is_empty() && log.drivers.is_empty(),
            "{garbage}"
        );
    }
}

#[test]
fn sysfs_tree_is_complete_and_wins() {
    let machine = thinkpad();
    let root = temp_root("thinkpad-sysfs");
    machine.write_sysfs_root(&root, true);
    // A stale Ultar log for one address must not override sysfs.
    write(
        &root,
        "proc/ultar/protokoll",
        "[treiber] 00:07.0 ffff:ffff -> nvme (hand) ok\n",
    );
    let inventory = HardwareReader::at(&root).inventory().unwrap();
    assert_eq!(inventory.devices.len(), 26);
    assert!(inventory.notes.is_empty());
    assert_eq!(inventory.machine.as_deref(), Some("ThinkPad P1 Gen 3"));
    assert_eq!(
        pci(find(&inventory.devices, "pci:00:07.0")),
        (0x144d, 0xa808, 0x01_0802, None)
    );
    let bluetooth = find(&inventory.devices, "usb:1-1");
    assert_eq!(
        bluetooth.primary,
        HardwareId::Usb {
            vendor: 0x8087,
            product: 0x0026,
            class: 0xe0_0101
        }
    );
}

#[test]
fn sysfs_details() {
    let root = temp_root("sysfs-details");
    let dev = "sys/bus/pci/devices";
    write(&root, &format!("{dev}/0001:02:00.0/vendor"), "0x10EC\n");
    write(&root, &format!("{dev}/0001:02:00.0/device"), "0x8168\n");
    write(&root, &format!("{dev}/0001:02:00.0/class"), "0x020000\n");
    write(
        &root,
        &format!("{dev}/0001:02:00.0/subsystem_vendor"),
        "0x1043\n",
    );
    write(
        &root,
        &format!("{dev}/0001:02:00.0/subsystem_device"),
        "0x8677\n",
    );
    write(&root, &format!("{dev}/0000:00:00.0/vendor"), "garbage\n");
    write(&root, &format!("{dev}/0000:00:00.0/device"), "0x0001\n");
    write(&root, &format!("{dev}/0000:00:00.0/class"), "0x060000\n");
    let usb = "sys/bus/usb/devices";
    write(&root, &format!("{usb}/usb1/idVendor"), "1d6b\n");
    write(&root, &format!("{usb}/usb1/idProduct"), "0002\n");
    write(&root, &format!("{usb}/1-2/idVendor"), "0bda\n");
    write(&root, &format!("{usb}/1-2/idProduct"), "8153\n");
    write(&root, &format!("{usb}/1-2:1.0/bInterfaceClass"), "ff\n");
    let inventory = HardwareReader::at(&root).inventory().unwrap();
    assert_eq!(inventory.devices.len(), 2);
    assert_eq!(
        pci(find(&inventory.devices, "pci:0001:02:00.0")),
        (0x10ec, 0x8168, 0x02_0000, Some((0x1043, 0x8677)))
    );
    assert_eq!(
        find(&inventory.devices, "usb:1-2").primary,
        HardwareId::Usb {
            vendor: 0x0bda,
            product: 0x8153,
            class: 0
        }
    );
    assert_eq!(inventory.machine, None);
}

#[test]
fn missing_and_unreadable_sources() {
    let root = temp_root("nothing");
    assert_eq!(
        HardwareReader::at(&root).inventory(),
        Err(HardwareError::NoSource)
    );
    assert_eq!(
        HardwareError::NoSource.to_string(),
        "hal.hardware.no-source"
    );
    // A directory where the log file should be is an I/O error, not "absent".
    std::fs::create_dir_all(root.join("proc/ultar/protokoll")).unwrap();
    let error = HardwareReader::at(&root).inventory().unwrap_err();
    assert!(matches!(error, HardwareError::Io { .. }), "{error}");
    assert_eq!(error.key(), "hal.hardware.io");
    assert_eq!(
        HardwareError::TooManyDevices.key(),
        "hal.hardware.too-many-devices"
    );
    for note in [
        InventoryNote::KernelLogWrapped,
        InventoryNote::BoundDevicesOnly,
        InventoryNote::ClassIncomplete(1),
        InventoryNote::PciCountMismatch {
            announced: 2,
            found: 1,
        },
    ] {
        assert!(note.key().starts_with("hal.hardware.note."));
    }
    assert_eq!(HardwareReader::running().root(), std::path::Path::new("/"));
}

#[test]
fn count_mismatch_is_noted() {
    let machine = beelink();
    let root = temp_root("mismatch");
    let log = machine.selftest_log().replace("[pci] 11 ", "[pci] 13 ");
    machine.write_ultar_root(&root, &log);
    let notes = HardwareReader::at(&root).inventory().unwrap().notes;
    assert!(notes.contains(&InventoryNote::PciCountMismatch {
        announced: 13,
        found: 11
    }));
}
