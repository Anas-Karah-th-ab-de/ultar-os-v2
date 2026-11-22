//! Test support: writes a Linux-style sysfs device tree for a [`Machine`].

use super::{write, KUsb, Machine};
use std::path::Path;

fn hex_file(root: &Path, dir: &str, name: &str, value: u32, digits: usize, prefix: &str) {
    let text = format!("{prefix}{value:0digits$x}\n");
    write(root, &format!("{dir}/{name}"), &text);
}

fn write_usb(root: &Path, u: &KUsb) {
    let dir = format!("sys/bus/usb/devices/1-{}", u.port);
    hex_file(root, &dir, "idVendor", u32::from(u.vendor), 4, "");
    hex_file(root, &dir, "idProduct", u32::from(u.product), 4, "");
    hex_file(root, &dir, "bDeviceClass", u.class >> 16, 2, "");
    hex_file(root, &dir, "bDeviceSubClass", (u.class >> 8) & 0xff, 2, "");
    hex_file(root, &dir, "bDeviceProtocol", u.class & 0xff, 2, "");
}

impl Machine {
    /// Writes a Linux-style sysfs tree below `root`. With `bound`, every device the Ultar
    /// kernel's rules would take gets a `driver` link of that name.
    pub fn write_sysfs_root(&self, root: &Path, bound: bool) {
        let model = format!("{}\n", self.model);
        write(root, "sys/class/dmi/id/product_name", &model);
        std::fs::create_dir_all(root.join("sys/bus/pci/drivers")).unwrap();
        for d in &self.pci {
            let dir = format!("sys/bus/pci/devices/0000:{}", d.slot);
            hex_file(root, &dir, "vendor", u32::from(d.vendor), 4, "0x");
            hex_file(root, &dir, "device", u32::from(d.device), 4, "0x");
            hex_file(root, &dir, "class", d.class, 6, "0x");
            hex_file(
                root,
                &dir,
                "subsystem_vendor",
                u32::from(d.subsys.0),
                4,
                "0x",
            );
            hex_file(
                root,
                &dir,
                "subsystem_device",
                u32::from(d.subsys.1),
                4,
                "0x",
            );
            if let (true, Some(Ok((name, _)))) = (bound, self.outcome(d)) {
                let driver = root.join("sys/bus/pci/drivers").join(name);
                std::fs::create_dir_all(&driver).unwrap();
                std::os::unix::fs::symlink(&driver, root.join(&dir).join("driver")).unwrap();
            }
        }
        for u in &self.usb {
            write_usb(root, u);
        }
    }
}
