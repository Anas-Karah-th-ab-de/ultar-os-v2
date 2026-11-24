//! Platform devices: the fixed devices of a PC that are not on PCI or USB and that the kernel
//! operates itself. ACPI devices are not enumerated (the kernel has no ACPI namespace interpreter).

use alloc::string::String;
use alloc::vec::Vec;

/// One platform device: Linux name, driver name.
#[derive(Clone, Copy, Debug)]
pub struct PlatformDev {
    pub name: &'static str,
    pub driver: &'static str,
}

/// Drivers of the platform devices (built in, part of the kernel core: never unbound).
pub const DRIVERS: [&str; 3] = ["efi-framebuffer", "i8042", "serial8250"];

/// Whether an i8042 keyboard controller answers (its status port reads `0xff` when absent).
fn i8042_present() -> bool {
    // SAFETY: reading the i8042 status port has no side effect; the port belongs to the kernel.
    unsafe { crate::arch::inb(0x64) != 0xff }
}

/// The platform devices of this machine.
pub fn list() -> Vec<PlatformDev> {
    let mut v = Vec::new();
    if crate::console::framebuffer().is_some() {
        v.push(PlatformDev { name: "efi-framebuffer.0", driver: "efi-framebuffer" });
    }
    if i8042_present() {
        v.push(PlatformDev { name: "i8042", driver: "i8042" });
    }
    // COM1 carries the kernel console and log
    v.push(PlatformDev { name: "serial8250", driver: "serial8250" });
    v
}

/// The device called `name`.
pub fn get(name: &str) -> Option<PlatformDev> {
    list().into_iter().find(|d| d.name == name)
}

/// `uevent` text of a platform device.
pub fn uevent(d: &PlatformDev) -> String {
    alloc::format!("DRIVER={}\nMODALIAS=platform:{}\n", d.driver, d.driver)
}
