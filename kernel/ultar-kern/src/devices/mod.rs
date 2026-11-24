//! Devices and drivers for user space: the device list, the driver register, binding control,
//! per-device probe and health results and hot-plug events (what `ultar-devd` needs).
//!
//! Interfaces (Linux layout where Linux has one, see `docs/integration/kernel-device-interfaces.md`):
//! * `/sys/bus/{pci,usb,platform}/devices`, `/sys/bus/*/drivers` ([`sysfs`]),
//! * `bind`/`unbind` of a PCI driver, `drivers_probe`, device `enable`, driver `ultar_enabled`
//!   (writing needs the right `LOAD_DRIVER`),
//! * uevents on an `AF_NETLINK`/`NETLINK_KOBJECT_UEVENT` socket with `SEQNUM` ([`events`]),
//! * `/sys/kernel/ultar/driver_abi`: the kernel driver ABI number ([`DRIVER_ABI`]).
//!
//! The PCI device table is `driver::DEVICES`; this module changes it only under [`CONTROL`].

pub mod events;
pub mod platform;
pub mod sysfs;
pub mod usb;

use crate::driver::{Driver, NetDev, Result as DrvResult, BLOCK, DEVICES, NET};
use crate::pci::PciDev;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// Number of the driver interface between this kernel and its built-in drivers (`driver.rs`).
/// It changes whenever a driver package built for the old one could no longer work.
pub const DRIVER_ABI: u32 = 1;

const EPERM: i64 = 1;
const EIO: i64 = 5;
const EBUSY: i64 = 16;
const ENODEV: i64 = 19;
const EINVAL: i64 = 22;
const EOPNOTSUPP: i64 = 95;

/// Class entries a driver created for one device: indices into `NET` and `BLOCK`. After an
/// unbind they stay reserved (a detached placeholder sits there), so the next bind gets the same
/// numbers and nobody else's index moves.
#[derive(Clone, Default, Debug)]
pub struct Slots {
    pub net: Vec<usize>,
    pub block: Vec<usize>,
}

/// One control operation at a time (bind, unbind, enable, hot-plug attach): the class entries a
/// probe creates are told apart by the length of the lists before and after it.
pub static CONTROL: Mutex<()> = Mutex::new(());

/// Drivers turned off through `ultar_enabled`: they bind nothing until turned on again.
static OFF: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// The place of a network interface whose driver was unbound: nothing goes in or out.
struct Detached(String);

impl NetDev for Detached {
    fn name(&self) -> String {
        self.0.clone()
    }
    fn mac(&self) -> [u8; 6] {
        [0; 6]
    }
    fn send(&mut self, _frame: &[u8]) -> DrvResult<()> {
        Err(String::from("device detached"))
    }
    fn recv(&mut self, _buf: &mut [u8]) -> Option<usize> {
        None
    }
    fn link(&self) -> Option<bool> {
        Some(false)
    }
    fn detached(&self) -> bool {
        true
    }
}

/// Run a driver's probe and note which class entries it created.
pub fn probe_with_slots(drv: &dyn Driver, d: &PciDev) -> (DrvResult<()>, Slots) {
    let (n0, b0) = (NET.lock().len(), BLOCK.lock().len());
    let r = drv.probe(d);
    let (n1, b1) = (NET.lock().len(), BLOCK.lock().len());
    (
        r,
        Slots {
            net: (n0..n1).collect(),
            block: (b0..b1).collect(),
        },
    )
}

/// Name of a PCI function as in Linux sysfs (`0000:00:03.0`).
pub fn pci_name(d: &PciDev) -> String {
    format!("0000:{:02x}:{:02x}.{}", d.bus, d.dev, d.func)
}

/// Driver by name.
pub fn driver(name: &str) -> Option<&'static dyn Driver> {
    crate::drivers::ALL
        .iter()
        .copied()
        .find(|d| d.name() == name)
}

/// Whether the driver is turned on.
pub fn driver_on(name: &str) -> bool {
    !OFF.lock().contains(&name)
}

/// Why a device cannot be unbound now; `None`: it can.
pub fn unbind_refusal(drv: &dyn Driver, slots: &Slots) -> Option<&'static str> {
    if let Err(why) = drv.can_remove() {
        return Some(why);
    }
    // block devices are held by the file systems by their index; nothing tells them to let go
    (!slots.block.is_empty()).then_some("block device held by the file system layer")
}

/// State word of a PCI function: `bound`, `unbound`, `failed` or `disabled`.
pub fn state(b: &crate::driver::Bound) -> &'static str {
    match (b.disabled, b.driver, &b.error) {
        (true, _, _) => "disabled",
        (false, Some(_), _) => "bound",
        (false, None, Some(_)) => "failed",
        (false, None, None) => "unbound",
    }
}

/// Index of the PCI function called `name` in `DEVICES`.
fn find(name: &str) -> Option<usize> {
    DEVICES.lock().iter().position(|b| pci_name(&b.dev) == name)
}

/// Put the entries a new probe appended (`fresh`) into the places the device had before
/// (`reserved`), so indices stay stable; returns the device's slots.
fn settle(fresh: Slots, reserved: &Slots) -> Slots {
    let mut nl = NET.lock();
    let k = fresh.net.len().min(reserved.net.len());
    for i in 0..k {
        nl.swap(reserved.net[i], fresh.net[i]);
    }
    // the placeholders now sit where the fresh entries were: at the end of the list
    if let Some(&first) = fresh.net.first() {
        nl.drain(first..first + k);
    }
    let mut net: Vec<usize> = reserved.net[..k].to_vec();
    net.extend(fresh.net.iter().skip(k).map(|i| i - k));
    Slots {
        net,
        block: fresh.block,
    }
}

/// Bind `driver` to the PCI function `name` (`/sys/bus/pci/drivers/<driver>/bind`).
pub fn bind(drv_name: &str, name: &str) -> Result<(), i64> {
    let _g = CONTROL.lock();
    let drv = driver(drv_name).ok_or(-ENODEV)?;
    let i = find(name).ok_or(-ENODEV)?;
    let (dev, reserved) = {
        let devs = DEVICES.lock();
        let b = &devs[i];
        if b.driver.is_some() {
            return Err(-EBUSY);
        }
        if b.disabled || !driver_on(drv_name) {
            return Err(-EPERM);
        }
        if !drv.matches(&b.dev) {
            return Err(-ENODEV);
        }
        (b.dev.clone(), b.slots.clone())
    };
    let (r, fresh) = crate::heap::kernel(|| probe_with_slots(drv, &dev));
    let slots = crate::heap::kernel(|| settle(fresh, &reserved));
    let ok = r.is_ok();
    {
        let mut devs = DEVICES.lock();
        let b = &mut devs[i];
        b.slots = slots;
        b.driver = ok.then_some(drv.name());
        b.tried = (!ok).then_some(drv.name());
        b.error = r.err();
    }
    if ok {
        crate::kprintln!("[devices] {name} -> {drv_name} bound");
        events::pci("bind", i);
        return Ok(());
    }
    crate::kprintln!("[devices] {name} -> {drv_name} probe failed");
    events::pci("change", i);
    Err(-EIO)
}

/// Bind the first matching, turned-on driver (`/sys/bus/pci/drivers_probe`).
pub fn probe_any(name: &str) -> Result<(), i64> {
    let dev = {
        let i = find(name).ok_or(-ENODEV)?;
        DEVICES.lock()[i].dev.clone()
    };
    let drv = crate::drivers::ALL
        .iter()
        .find(|d| driver_on(d.name()) && d.matches(&dev))
        .ok_or(-ENODEV)?;
    bind(drv.name(), name)
}

/// Detach the network entries of a device: placeholders take their places (under the list's lock,
/// so no other CPU is inside the driver's code when it is dropped).
fn detach(slots: &Slots) -> Vec<Box<dyn NetDev>> {
    let mut nl = NET.lock();
    let mut gone = Vec::new();
    for &s in &slots.net {
        if let Some(e) = nl.get_mut(s) {
            let name = e.name();
            gone.push(core::mem::replace(e, Box::new(Detached(name))));
        }
    }
    gone
}

/// Unbind the driver of the PCI function `name`. The device is quiesced by its driver (DMA
/// stopped, device reset), then bus mastering and decoding are turned off. A driver without a
/// teardown path is not touched: `EOPNOTSUPP`; a device held by the file systems: `EBUSY`.
pub fn unbind(name: &str) -> Result<(), i64> {
    let _g = CONTROL.lock();
    unbind_locked(name)
}

fn unbind_locked(name: &str) -> Result<(), i64> {
    let i = find(name).ok_or(-ENODEV)?;
    let (dev, drv, slots) = {
        let devs = DEVICES.lock();
        let b = &devs[i];
        let drv = b.driver.and_then(driver).ok_or(-ENODEV)?;
        match unbind_refusal(drv, &b.slots) {
            Some(why) if why.starts_with("block") => return Err(-EBUSY),
            Some(_) => return Err(-EOPNOTSUPP),
            None => {}
        }
        (b.dev.clone(), drv, b.slots.clone())
    };
    let gone = crate::heap::kernel(|| detach(&slots));
    let r = drv.remove(&dev);
    drop(gone);
    // no more DMA, no more register decoding, whatever state the device is in
    let cmd = dev.r32(0x04);
    dev.w32(0x04, cmd & !0x7);
    {
        let mut devs = DEVICES.lock();
        let b = &mut devs[i];
        b.driver = None;
        b.tried = r.is_err().then_some(drv.name());
        b.error = r.as_ref().err().map(|e| format!("teardown: {e}"));
    }
    crate::kprintln!("[devices] {name}: {} unbound", drv.name());
    events::pci("unbind", i);
    r.map_err(|_| -EIO)
}

/// Turn a PCI function off (unbinding its driver) or on again (`/sys/bus/pci/devices/<d>/enable`).
pub fn set_device_enabled(name: &str, on: bool) -> Result<(), i64> {
    let _g = CONTROL.lock();
    let i = find(name).ok_or(-ENODEV)?;
    if !on {
        let (bound, dev) = {
            let devs = DEVICES.lock();
            (devs[i].driver.is_some(), devs[i].dev.clone())
        };
        if bound {
            unbind_locked(name)?;
        }
        let cmd = dev.r32(0x04);
        dev.w32(0x04, cmd & !0x7);
    }
    DEVICES.lock()[i].disabled = !on;
    events::pci("change", i);
    Ok(())
}

/// Turn a driver off (unbinding it from every device first; nothing changes if one of them
/// cannot be unbound) or on (`/sys/bus/pci/drivers/<d>/ultar_enabled`).
pub fn set_driver_enabled(drv_name: &str, on: bool) -> Result<(), i64> {
    let drv = driver(drv_name).ok_or(-ENODEV)?;
    let _g = CONTROL.lock();
    if on {
        OFF.lock().retain(|n| *n != drv_name);
        return Ok(());
    }
    let mine: Vec<String> = {
        let devs = DEVICES.lock();
        for b in devs.iter().filter(|b| b.driver == Some(drv.name())) {
            if unbind_refusal(drv, &b.slots).is_some() {
                return Err(-EOPNOTSUPP);
            }
        }
        devs.iter()
            .filter(|b| b.driver == Some(drv.name()))
            .map(|b| pci_name(&b.dev))
            .collect()
    };
    for name in mine {
        unbind_locked(&name)?;
    }
    let mut off = OFF.lock();
    if !off.contains(&drv.name()) {
        off.push(drv.name());
    }
    Ok(())
}

/// Parse a `0`/`1` control value.
pub fn flag(text: &str) -> Result<bool, i64> {
    match text {
        "1" => Ok(true),
        "0" => Ok(false),
        _ => Err(-EINVAL),
    }
}

/// Probe and health result of the PCI function at `DEVICES[i]`, as `key=value` lines:
/// `probe` (ok, failed, none), `error`, `link` per network interface, `media` per block device
/// (sector 0 read now), `bus_master`.
pub fn health(i: usize) -> Option<String> {
    let (probe, err, slots, cmd) = {
        let devs = DEVICES.lock();
        let b = devs.get(i)?;
        let probe = match (b.driver, &b.error) {
            (Some(_), _) => "ok",
            (None, Some(_)) => "failed",
            (None, None) => "none",
        };
        (
            probe,
            b.error.clone(),
            if b.driver.is_some() {
                b.slots.clone()
            } else {
                Slots::default()
            },
            b.dev.r32(0x04),
        )
    };
    let mut s = format!("probe={probe}\n");
    if let Some(e) = err {
        s += &format!("error={}\n", e.replace('\n', " "));
    }
    for n in &slots.net {
        let link = NET.lock().get(*n).and_then(|d| d.link());
        s += &format!(
            "link={}\n",
            match link {
                Some(true) => "up",
                Some(false) => "down",
                None => "unknown",
            }
        );
    }
    for k in &slots.block {
        s += &format!("media={}\n", media(*k));
    }
    s += &format!("bus_master={}\n", if cmd & 0x4 != 0 { "on" } else { "off" });
    Some(s)
}

/// Read the first sector of block device `k`.
fn media(k: usize) -> &'static str {
    let mut bl = BLOCK.lock();
    let Some(d) = bl.get_mut(k) else {
        return "missing";
    };
    let mut buf = alloc::vec![0u8; d.sector_size()];
    match d.read(0, &mut buf) {
        Ok(()) => "ok",
        Err(_) => "failed",
    }
}

/// Called often from the kernel's polling paths: USB hot-plug (at most every 200 ms).
pub fn poll() {
    usb::poll();
}
