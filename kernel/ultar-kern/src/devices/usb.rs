//! USB devices behind the xHCI controllers, as the controller driver attached them (at boot and on
//! hot-plug). Names as in Linux: `<bus>-<port>[.<port>…]` for a device (bus = controller number
//! from 1), `<device>:1.<interface>` for its interfaces.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// One interface of a USB device and the function driver of the xHCI driver that serves it.
#[derive(Clone, Debug)]
pub struct Iface {
    pub number: u8,
    pub class: u8,
    pub sub: u8,
    pub proto: u8,
    /// `hub`, `usbhid`, `usb-storage`; `None`: nothing serves it.
    pub driver: Option<&'static str>,
}

/// One attached USB device.
#[derive(Clone, Debug)]
pub struct UsbDev {
    /// Controller index (bus number − 1).
    pub hc: usize,
    /// Port path below the controller (`2`, `2.1`).
    pub port: String,
    /// Device slot of the controller.
    pub slot: u32,
    pub vendor: u16,
    pub product: u16,
    pub bcd: u16,
    /// Class, subclass, protocol of the device descriptor.
    pub class: [u8; 3],
    /// Link speed code of the port (xHCI: 1 full, 2 low, 3 high, 4+ super speed).
    pub speed: u32,
    pub ifaces: Vec<Iface>,
}

impl UsbDev {
    /// Linux name (`1-2.1`).
    pub fn name(&self) -> String {
        format!("{}-{}", self.hc + 1, self.port)
    }
}

/// Function drivers of the xHCI driver (listed below `/sys/bus/usb/drivers`).
pub const DRIVERS: [&str; 3] = ["hub", "usb-storage", "usbhid"];

static DEVS: Mutex<Vec<UsbDev>> = Mutex::new(Vec::new());
static NEXT_POLL: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// All attached devices, ordered by name.
pub fn list() -> Vec<UsbDev> {
    let mut v = DEVS.lock().clone();
    v.sort_by_key(|d| d.name());
    v
}

/// The controller driver attached a device.
pub fn added(d: UsbDev) {
    let vars = vars(&d);
    let path = format!("/bus/usb/devices/{}", d.name());
    crate::heap::kernel(|| DEVS.lock().push(d));
    super::events::emit("add", &path, "usb", &vars);
}

/// The device at port path `port` of controller `hc` and everything behind it are gone; returns
/// their slots (the controller driver frees them).
pub fn removed(hc: usize, port: &str) -> Vec<u32> {
    let below =
        |d: &UsbDev| d.hc == hc && (d.port == port || d.port.starts_with(&format!("{port}.")));
    let gone: Vec<UsbDev> = {
        let mut list = DEVS.lock();
        let gone = list.iter().filter(|d| below(d)).cloned().collect();
        list.retain(|d| !below(d));
        gone
    };
    for d in gone.iter().rev() {
        super::events::emit(
            "remove",
            &format!("/bus/usb/devices/{}", d.name()),
            "usb",
            &vars(d),
        );
    }
    gone.iter().map(|d| d.slot).collect()
}

/// Whether a device is attached at a root port of a controller.
pub fn present(hc: usize, port: u32) -> bool {
    let p = format!("{port}");
    DEVS.lock().iter().any(|d| d.hc == hc && d.port == p)
}

fn vars(d: &UsbDev) -> Vec<(&'static str, String)> {
    alloc::vec![
        ("DEVTYPE", String::from("usb_device")),
        (
            "PRODUCT",
            format!("{:x}/{:x}/{:x}", d.vendor, d.product, d.bcd)
        ),
        (
            "TYPE",
            format!("{}/{}/{}", d.class[0], d.class[1], d.class[2])
        ),
        ("BUSNUM", format!("{:03}", d.hc + 1)),
        ("DEVNUM", format!("{:03}", d.slot)),
    ]
}

/// Hot-plug: look at the root ports of the controllers, at most every 200 ms and never while a
/// bind or unbind runs.
pub fn poll() {
    let now = crate::arch::uptime_ms();
    if now < NEXT_POLL.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    NEXT_POLL.store(now + 200, core::sync::atomic::Ordering::Relaxed);
    let Some(_g) = super::CONTROL.try_lock() else {
        return;
    };
    crate::heap::kernel(crate::drivers::gen_xhci::hotplug);
}
