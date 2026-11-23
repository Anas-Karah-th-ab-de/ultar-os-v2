//! Hardware inventory from the sysfs device tree (Ultar kernel and Linux).
//!
//! The Ultar tree is produced by `common/kernel_fmt.rs` / `sysfs_tree.rs`, which copy the
//! formats of the kernel's `devices/sysfs.rs`.

#[path = "common/kernel_fmt.rs"]
mod kernel_fmt;

use kernel_fmt::{beelink, temp_root, thinkpad, write};
use ultar_driverpkg::{DeviceDescriptor, HardwareId};
use ultar_hal_system::{HardwareError, HardwareReader};

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
fn ultar_kernel_tree_gives_every_device_with_class() {
    let machine = beelink();
    let root = temp_root("beelink-ultar");
    machine.write_ultar_root(&root);
    let inventory = HardwareReader::at(&root).inventory().unwrap();
    assert_eq!(
        inventory.devices.len(),
        machine.pci.len() + machine.usb.len()
    );
    assert!(inventory.sources.sysfs_pci);
    for d in &machine.pci {
        let found = find(&inventory.devices, &d.address());
        assert_eq!(pci(found).2, d.class, "{}", d.address());
    }
    assert_eq!(inventory.machine.as_deref(), Some(machine.model.as_str()));
}

#[test]
fn sysfs_tree_is_complete_and_wins() {
    let machine = thinkpad();
    let root = temp_root("thinkpad-sysfs");
    machine.write_sysfs_root(&root, true);
    let inventory = HardwareReader::at(&root).inventory().unwrap();
    assert_eq!(inventory.devices.len(), 26);
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
    // A file where the device directory should be is an I/O error, not "absent".
    write(&root, "sys/bus/pci/devices", "not a directory\n");
    let error = HardwareReader::at(&root).inventory().unwrap_err();
    assert!(matches!(error, HardwareError::Io { .. }), "{error}");
    assert_eq!(error.key(), "hal.hardware.io");
    assert_eq!(
        HardwareError::TooManyDevices.key(),
        "hal.hardware.too-many-devices"
    );
    assert_eq!(HardwareReader::running().root(), std::path::Path::new("/"));
}
