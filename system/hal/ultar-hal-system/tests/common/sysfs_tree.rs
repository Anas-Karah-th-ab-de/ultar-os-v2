//! Test support: writes a Linux-style sysfs device tree for a [`Machine`].

use super::{write, KDev, KUsb, Machine};
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

/// `drivers::ALL` of the Ultar kernel: name, origin, whether it has a teardown path.
pub const ULTAR_DRIVERS: &[(&str, &str, bool)] = &[
    ("virtio-blk", "hand", false),
    ("virtio-net", "hand", true),
    ("nvme", "hand", false),
    ("virtio-rng", "hand", false),
    ("bruecke", "hand", false),
    ("gen-ahci", "werkstatt", false),
    ("gen-bochs-vga", "werkstatt", false),
    ("gen-hda", "werkstatt", false),
    ("gen-intel-eth", "werkstatt", true),
    ("gen-ixgbe", "werkstatt", false),
    ("gen-smbus", "werkstatt", false),
    ("gen-xhci", "werkstatt", false),
];

impl Machine {
    /// The sysfs tree of the Ultar kernel (`devices/sysfs.rs`): the Linux files plus
    /// `ultar_*`, the driver register with `bind`/`unbind`, `driver` links as relative links.
    pub fn write_ultar_sysfs(&self, root: &Path) {
        for (name, origin, unbind) in ULTAR_DRIVERS {
            let dir = format!("sys/bus/pci/drivers/{name}");
            for file in ["bind", "unbind"] {
                write(root, &format!("{dir}/{file}"), "");
            }
            write(root, &format!("{dir}/ultar_origin"), &format!("{origin}\n"));
            write(root, &format!("{dir}/ultar_enabled"), "1\n");
            let word = if *unbind {
                "supported\n"
            } else {
                "unsupported: no teardown path in this driver\n"
            };
            write(root, &format!("{dir}/ultar_unbind"), word);
        }
        for d in &self.pci {
            self.write_ultar_pci(root, d);
        }
        for u in &self.usb {
            write_usb(root, u);
        }
    }

    /// One PCI function: ids, bound driver link, probe/state/health and candidate drivers.
    fn write_ultar_pci(&self, root: &Path, d: &KDev) {
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
        let (probe, state) = match self.outcome(d) {
            Some(Ok((name, _))) => {
                std::os::unix::fs::symlink(
                    format!("../../drivers/{name}"),
                    root.join(&dir).join("driver"),
                )
                .unwrap();
                ("ok".to_string(), "bound")
            }
            Some(Err(e)) => (
                format!(
                    "failed: {} {e}",
                    super::kernel_driver(d).map_or("-", |x| x.0)
                ),
                "failed",
            ),
            None => ("none".to_string(), "unbound"),
        };
        write(root, &format!("{dir}/ultar_probe"), &format!("{probe}\n"));
        write(root, &format!("{dir}/ultar_state"), &format!("{state}\n"));
        let health = if state == "bound" {
            "probe=ok\nbus_master=on\n"
        } else {
            "probe=none\nbus_master=off\n"
        };
        write(root, &format!("{dir}/ultar_health"), health);
        let drivers = super::kernel_driver(d)
            .map(|x| format!("{}\n", x.0))
            .unwrap_or_default();
        write(root, &format!("{dir}/ultar_drivers"), &drivers);
    }
}
