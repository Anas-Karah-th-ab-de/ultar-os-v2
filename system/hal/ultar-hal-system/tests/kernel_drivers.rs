//! Built-in kernel drivers, bindings and control files from the sysfs tree (Ultar, Linux).

#[path = "common/kernel_fmt.rs"]
mod kernel_fmt;

use kernel_fmt::{beelink, temp_root, thinkpad, write, ULTAR_VERSION};
use ultar_hal_system::kernel_drivers::{sysfs_pci_name, ULTAR_KERNEL_ABI};
use ultar_hal_system::{kernel_abi, BindState, ControlInterface, HardwareReader, KernelDrivers};

fn bound(driver: &str) -> BindState {
    BindState::Bound {
        driver: driver.into(),
    }
}

#[test]
fn ultar_tree_bindings_register_and_control() {
    let mut machine = thinkpad();
    // 00:07.0 is the NVMe drive of the fixture.
    machine
        .probe_errors
        .insert("00:07.0".into(), "BAR0 missing".into());
    let root = temp_root("kd-thinkpad");
    machine.write_ultar_root(&root);
    let drivers = KernelDrivers::read(&HardwareReader::at(&root)).unwrap();
    assert_eq!(drivers.control, ControlInterface::SysfsBind);
    assert!(drivers.register_complete);
    assert_eq!(drivers.binding("pci:00:08.0"), bound("gen-intel-eth"));
    assert_eq!(drivers.binding("pci:00:00.0"), bound("bruecke"));
    assert_eq!(
        drivers.binding("pci:00:07.0"),
        BindState::Failed {
            driver: Some("nvme".into()),
            error: "BAR0 missing".into()
        }
    );
    // Intel cAVS audio has class 04.01: the kernel's gen-hda (class 04.03) does not take it.
    assert_eq!(drivers.binding("pci:00:0e.0"), BindState::Unbound);
    assert_eq!(drivers.binding("pci:ff:00.0"), BindState::Unknown);
    let xhci = drivers.driver("gen-xhci").unwrap();
    assert_eq!(xhci.origin.as_deref(), Some("werkstatt"));
    assert_eq!(xhci.unbind, Some(false));
    assert_eq!(drivers.driver("gen-intel-eth").unwrap().unbind, Some(true));
    assert_eq!(drivers.driver("nvme").unwrap().failed, ["pci:00:07.0"]);
    assert_eq!(drivers.exists("gen-ahci"), Some(true));
    assert_eq!(drivers.exists("intel-wifi"), Some(false));
    assert_eq!(
        drivers.health("pci:00:08.0"),
        Some("probe=ok\nbus_master=on\n")
    );
    assert_eq!(
        drivers.candidates("pci:00:08.0").unwrap(),
        ["gen-intel-eth"]
    );
    // the control files take the Linux device name
    drivers.unbind_device("pci:00:08.0").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("sys/bus/pci/drivers/gen-intel-eth/unbind")).unwrap(),
        "0000:00:08.0"
    );
    drivers.bind_device("gen-intel-eth", "pci:00:08.0").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("sys/bus/pci/drivers/gen-intel-eth/bind")).unwrap(),
        "0000:00:08.0"
    );
    assert!(drivers.bind_device("gen-intel-eth", "usb:1-1").is_err());
}

#[test]
fn sysfs_bindings_and_control_files() {
    let machine = beelink();
    let root = temp_root("kd-sysfs");
    machine.write_sysfs_root(&root, true);
    let reader = HardwareReader::at(&root);
    let drivers = KernelDrivers::read(&reader).unwrap();
    assert!(drivers.register_complete);
    assert_eq!(drivers.control, ControlInterface::None);
    assert_eq!(drivers.binding("pci:01:00.0"), bound("nvme"));
    assert_eq!(drivers.binding("pci:ac:00.0"), BindState::Unbound);
    assert_eq!(drivers.binding("usb:1-1"), BindState::Unbound);
    assert_eq!(drivers.exists("e1000e"), Some(false));
    // Linux offers bind/unbind files; a USB interface carries the driver link.
    write(&root, "sys/bus/pci/drivers/nvme/bind", "");
    write(&root, "sys/bus/pci/drivers/nvme/unbind", "");
    std::fs::create_dir_all(root.join("sys/bus/usb/devices/1-2:1.0")).unwrap();
    std::os::unix::fs::symlink(
        root.join("sys/bus/usb/drivers/btusb"),
        root.join("sys/bus/usb/devices/1-2:1.0/driver"),
    )
    .unwrap();
    let drivers = KernelDrivers::read(&reader).unwrap();
    assert_eq!(drivers.control, ControlInterface::SysfsBind);
    assert_eq!(drivers.binding("usb:1-2"), bound("btusb"));
    assert_eq!(drivers.driver("btusb").unwrap().bound, ["usb:1-2"]);
}

#[test]
fn kernel_abi_detection() {
    let root = temp_root("abi");
    assert_eq!(kernel_abi(&root).unwrap(), None);
    write(&root, "proc/sys/kernel/osrelease", "6.8.0-48-generic\n");
    assert_eq!(
        kernel_abi(&root).unwrap().as_deref(),
        Some("linux-6.8.0-48-generic")
    );
    write(
        &root,
        "proc/version",
        "Linux version 6.8.0-48-generic (buildd@lcy02)\n",
    );
    assert_eq!(
        kernel_abi(&root).unwrap().as_deref(),
        Some("linux-6.8.0-48-generic")
    );
    // /proc/version alone proves nothing: the ABI number comes from the kernel
    write(&root, "proc/version", ULTAR_VERSION);
    assert_eq!(
        kernel_abi(&root).unwrap().as_deref(),
        Some("linux-6.8.0-48-generic")
    );
    write(&root, "sys/kernel/ultar/driver_abi", "1\n");
    assert_eq!(
        kernel_abi(&root).unwrap().as_deref(),
        Some(ULTAR_KERNEL_ABI)
    );
    write(&root, "sys/kernel/ultar/driver_abi", "2\n");
    assert_eq!(
        kernel_abi(&root).unwrap().as_deref(),
        Some("ultar-kern-abi-2")
    );
    assert_eq!(
        sysfs_pci_name("pci:00:08.0").as_deref(),
        Some("0000:00:08.0")
    );
    assert_eq!(
        sysfs_pci_name("pci:0001:02:00.0").as_deref(),
        Some("0001:02:00.0")
    );
    assert_eq!(sysfs_pci_name("usb:1-2"), None);
    let empty = temp_root("abi-empty");
    write(&empty, "proc/sys/kernel/osrelease", "\n");
    assert_eq!(kernel_abi(&empty).unwrap(), None);
}

#[test]
fn empty_root_is_an_empty_snapshot() {
    let root = temp_root("kd-empty");
    let drivers = KernelDrivers::read(&HardwareReader::at(&root)).unwrap();
    assert_eq!(drivers.device_count(), 0);
    assert_eq!(drivers.exists("nvme"), None);
    assert_eq!(drivers.drivers().count(), 0);
}
