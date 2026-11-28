//! Hot-plug of the xHCI driver: devices that appear or leave at a root port after boot, found by
//! the port's "connect status change" bit (the driver polls, it uses no interrupts), and the
//! registration of every attached device in the kernel's device list (`devices::usb`).
//!
//! Leaving: the device slots of the device and of everything behind it (hubs) are disabled and
//! their transfer rings dropped; a keyboard stops delivering keys. A USB disk's block device
//! entry stays and fails every access (the file systems hold it by index).

use super::{attach, enable_port, r32, w32, w64, Desc, Hc, Lage, HC};
use crate::devices::usb::{Iface, UsbDev};
use alloc::format;
use alloc::vec::Vec;

const TRB_DISABLE_SLOT: u32 = 10;
/// PORTSC: current connect status, connect status change.
const CCS: u32 = 1;
const CSC: u32 = 1 << 17;

/// The function driver that serves an interface (the same tests as in `attach`).
fn function(d: &Desc) -> Option<&'static str> {
    match (d.class, d.sub, d.proto) {
        (9, _, _) => Some("hub"),
        (3, 1, 1) => Some("usbhid"),
        (8, 6, 0x50) => Some("usb-storage"),
        (0xE0, 1, 1) => Some("btusb"),
        (0x0E, 1, _) => Some("uvcvideo"),
        (0x02, 2, 0xFF) => Some("rndis"), // RNDIS-Steuerung (USB-Netz)
        (0x0A, _, _) => Some("usb-net"),  // CDC-Daten (RNDIS/ECM)
        _ => None,
    }
}

/// Note an attached device in the kernel's device list (from `attach`, device descriptor `dd`).
pub(super) fn register(hc: &Hc, lage: &Lage, slot: u32, speed: u32, dd: &[u8], ifs: &[Desc]) {
    let (Some(vid), Some(pid), Some(bcd)) = (dd.get(8..10), dd.get(10..12), dd.get(12..14)) else { return };
    let le = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]);
    let ifaces: Vec<Iface> = ifs.iter().map(|d| Iface { number: d.iface, class: d.class, sub: d.sub, proto: d.proto, driver: function(d) }).collect();
    if hc.kbds.iter().any(|k| k.slot == slot) {
        crate::input::usb_keyboard_added(hc.index, slot, le(vid), le(pid));
    }
    if let Some(abs) = super::pointer::ranges(hc, slot) {
        crate::input::usb_pointer_added(hc.index, slot, le(vid), le(pid), abs);
    }
    crate::devices::usb::added(UsbDev {
        hc: hc.index,
        port: lage.name(),
        slot,
        vendor: le(vid),
        product: le(pid),
        bcd: le(bcd),
        class: [dd[4], dd[5], dd[6]],
        speed,
        ifaces,
    });
}

/// Free the slots of the device at root port `port` and of everything behind it.
fn detach(hc: &mut Hc, port: u32) {
    for slot in crate::devices::usb::removed(hc.index, &format!("{port}")) {
        hc.kbds.retain(|k| k.slot != slot);
        super::pointer::remove(hc, slot);
        super::bt::remove(hc, slot);
        super::uvc::remove(hc, slot);
        crate::input::usb_removed(hc.index, slot);
        hc.rings.retain(|r| r.0 != slot);
        hc.command(0, (TRB_DISABLE_SLOT << 10) | (slot << 24));
        w64(hc.dcbaa + slot as u64 * 8, 0);
    }
}

/// Look at every root port whose connection changed: attach what came, detach what left.
pub fn hotplug() {
    let Some(mut g) = HC.try_lock() else { return };
    for hc in g.iter_mut() {
        for p in 1..=hc.ports {
            let pr = hc.op + 0x400 + (p as u64 - 1) * 0x10;
            let sc = r32(pr);
            if sc & CSC == 0 {
                continue;
            }
            // acknowledge the change; keep power, clear no other change bit by accident
            w32(pr, (sc & 0x0e00_c3e0 & !(1 << 1)) | CSC);
            if crate::devices::usb::present(hc.index, p) {
                crate::kprintln!("[xhci] port {p}: device left");
                detach(hc, p);
            }
            if sc & CCS == 0 {
                continue;
            }
            crate::arch::sleep_us(100_000);
            if let Some(speed) = enable_port(hc, p) {
                match attach(hc, Lage::wurzel(p), speed) {
                    Ok(_) => crate::kprintln!("[xhci] port {p}: device attached"),
                    Err(_) => crate::kprintln!("[xhci] port {p}: attach failed"),
                }
            }
        }
    }
}
