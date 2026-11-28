//! Input event nodes (`/dev/input/eventN`) as on Linux: `event0` is the PS/2 mouse (absolute pixel
//! axes) and `event1` the PS/2 keyboard (both in `eingabe.rs`); every USB boot keyboard, mouse and
//! absolute tablet gets its own node from `event2` on while it is attached (hot-plug): keyboards
//! with real key-down and key-up events from their HID reports, mice with relative motion, tablets
//! with absolute X/Y in the range of their report descriptor, both with buttons and the wheel.
//!
//! A node that comes or goes is announced by a device event (`SUBSYSTEM=input`,
//! `DEVNAME=input/eventN`); `/sys/class/input/eventN` describes it (`device/name`, `dev`,
//! `uevent`). A descriptor of a node whose keyboard left reads `ENODEV`. While no program reads an
//! input node, USB keys go to the console as characters (see `gen_xhci`).
//!
//! Virtual devices of privileged services (`/dev/ultar/uinput`, `svcdev/uinput.rs`) get nodes the
//! same way: they answer with the capabilities they declared (`virt.rs`) and only carry events
//! of those; at most [`MAX_VIRTUAL`] exist at once.

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

pub mod virt;

/// The first number of a USB keyboard's node.
pub const FIRST: u8 = 2;
/// Most USB keyboard nodes at once.
pub const MAX_NODES: usize = 32;
/// Most virtual devices at once.
pub const MAX_VIRTUAL: usize = 8;
/// Controller number of virtual devices (their `slot` is a serial number).
const VIRTUAL_HC: usize = usize::MAX;
/// Events kept per node until a program reads them.
const QUEUE: usize = 4096;
const ENODEV: i64 = 19;
/// `BUS_USB`, `BUS_I8042` of `struct input_id`.
const BUS_USB: u16 = 3;
const BUS_I8042: u16 = 0x11;
/// `BUS_VIRTUAL`: every device a service makes through `uinput` reports it, whatever the service
/// declared – the seat (devd, the compositor) tells it from hardware by this (review S-4).
pub const BUS_VIRTUAL: u16 = 6;

/// HID usage (keyboard page) → Linux key code, usages 0x00..=0x65 (as Linux' `usbkbd`).
const HID_KEYS: [u8; 0x66] = [
    0, 0, 0, 0, 30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 28, 1, 14, 15, 57, 12, 13, 26, 27, 43, 43, 39, 40, 41, 51, 52,
    53, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 87, 88, 99, 70, 119, 110, 102, 104, 111, 107, 109, 106, 105, 108, 103, 69, 98, 55, 74, 78, 96, 79, 80, 81, 75, 76, 77, 71, 72, 73, 82, 83, 86, 127,
];
/// Modifier bits of a boot report → Linux key codes (left Ctrl, Shift, Alt, Meta, then right).
const MODIFIERS: [u16; 8] = [29, 42, 56, 125, 97, 54, 100, 126];

/// What a USB node is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Keyboard,
    Mouse,
    /// Absolute X and Y: minimum and maximum of each.
    Tablet([i32; 4]),
    /// A virtual device of a service (capabilities in the node).
    Virtual,
}

struct Node {
    index: u8,
    kind: Kind,
    hc: usize,
    slot: u32,
    vendor: u16,
    product: u16,
    /// The last boot report (modifiers, reserved, six keys).
    last: [u8; 8],
    /// One queue per reader (open descriptor), as Linux' evdev clients: every reader gets every
    /// event. A single shared queue let any second reader (ultar-init's emergency key watch as
    /// PID 1) take events away from the compositor - found in VMware: no key reached the session.
    queues: Vec<(u32, VecDeque<[u8; 24]>)>,
    /// What a virtual device declared (one entry; empty for USB devices – a vector so that its
    /// memory can be asked for without stopping the machine when there is none).
    caps: Vec<virt::Caps>,
}

static NODES: Mutex<Vec<Node>> = Mutex::new(Vec::new());

/// A USB boot keyboard was attached (slot `slot` of controller `hc`): it gets the lowest free node.
pub fn usb_keyboard_added(hc: usize, slot: u32, vendor: u16, product: u16) {
    add(hc, slot, vendor, product, Kind::Keyboard);
}

/// A USB mouse (`abs` = `None`) or tablet (`abs` = ranges of X and Y) was attached.
pub fn usb_pointer_added(hc: usize, slot: u32, vendor: u16, product: u16, abs: Option<[i32; 4]>) {
    add(hc, slot, vendor, product, abs.map_or(Kind::Mouse, Kind::Tablet));
}

fn add(hc: usize, slot: u32, vendor: u16, product: u16, kind: Kind) {
    let index = {
        let mut nodes = NODES.lock();
        if nodes.len() >= MAX_NODES || nodes.iter().any(|n| n.hc == hc && n.slot == slot && n.kind == kind) {
            return;
        }
        let Some(index) = (FIRST..=u8::MAX).find(|i| !nodes.iter().any(|n| n.index == *i)) else { return };
        let node = Node { index, kind, hc, slot, vendor, product, last: [0; 8], queues: Vec::new(), caps: Vec::new() };
        if !crate::heap::kernel(|| crate::mem::try_reserve(&mut nodes, 1)) {
            return;
        }
        nodes.push(node);
        index
    };
    crate::kprintln!("[input] event{index}: USB {} {vendor:04x}:{product:04x}", word(kind));
    uevent(index, "add");
}

fn word(kind: Kind) -> &'static str {
    match kind {
        Kind::Keyboard => "Keyboard",
        Kind::Mouse => "Mouse",
        Kind::Tablet(_) => "Tablet",
        Kind::Virtual => "Virtual",
    }
}

static NEXT_VIRTUAL: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);

/// A service created a virtual device with `caps`: its node number (`None`: too many devices or
/// no memory).
pub fn virtual_added(caps: virt::Caps) -> Option<u8> {
    let index = {
        let mut nodes = NODES.lock();
        if nodes.len() >= MAX_NODES || nodes.iter().filter(|n| n.kind == Kind::Virtual).count() >= MAX_VIRTUAL {
            return None;
        }
        let index = (FIRST..=u8::MAX).find(|i| !nodes.iter().any(|n| n.index == *i))?;
        let slot = NEXT_VIRTUAL.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let (vendor, product) = (caps.id[1], caps.id[2]);
        let mut held = Vec::new();
        if !crate::heap::kernel(|| crate::mem::try_reserve(&mut held, 1)) {
            return None;
        }
        held.push(caps);
        let node = Node { index, kind: Kind::Virtual, hc: VIRTUAL_HC, slot, vendor, product, last: [0; 8], queues: Vec::new(), caps: held };
        if !crate::heap::kernel(|| crate::mem::try_reserve(&mut nodes, 1)) {
            return None;
        }
        nodes.push(node);
        index
    };
    crate::kprintln!("[input] event{index}: virtual device");
    uevent(index, "add");
    Some(index)
}

/// The virtual device of node `index` was destroyed (its descriptor closed).
pub fn virtual_removed(index: u8) {
    let gone = {
        let mut nodes = NODES.lock();
        let before = nodes.len();
        nodes.retain(|n| !(n.index == index && n.kind == Kind::Virtual));
        nodes.len() != before
    };
    if gone {
        crate::perm::forget(&format!("/dev/input/event{index}"));
        crate::kprintln!("[input] event{index}: removed");
        uevent(index, "remove");
    }
}

/// Events `(type, code, value)` of the virtual device of node `index`: all of them were declared
/// (else nothing is queued and `false` returned). Time stamps are the kernel's.
pub fn virtual_events(index: u8, events: &[(u16, u16, i32)]) -> bool {
    let mut nodes = NODES.lock();
    let Some(n) = nodes.iter_mut().find(|n| n.index == index && n.kind == Kind::Virtual) else { return false };
    let Some(caps) = n.caps.first() else { return false };
    let mut ok = Vec::new();
    if crate::heap::kernel(|| ok.try_reserve(events.len()).is_err()) {
        return false;
    }
    for &(t, c, v) in events {
        let Some(v) = caps.filter(t, c, v) else { return false };
        ok.push((t, c, v));
    }
    for (t, c, v) in ok {
        push(n, t, c, v);
    }
    crate::socket::bump();
    true
}

/// The USB device in slot `slot` of controller `hc` left: its nodes go away.
pub fn usb_removed(hc: usize, slot: u32) {
    let gone: Vec<u8> = {
        let mut nodes = NODES.lock();
        let gone = nodes.iter().filter(|n| n.hc == hc && n.slot == slot).map(|n| n.index).collect();
        nodes.retain(|n| !(n.hc == hc && n.slot == slot));
        gone
    };
    for index in gone {
        // a later keyboard under the same number starts with the default owner and mode again
        crate::perm::forget(&format!("/dev/input/event{index}"));
        crate::kprintln!("[input] event{index}: removed");
        uevent(index, "remove");
    }
}

/// A boot report of the keyboard in `slot`; `raw`: a program reads input nodes. Returns whether the
/// keys went to the keyboard's node (else the caller hands them to the console).
pub fn usb_report(hc: usize, slot: u32, rep: &[u8; 8], raw: bool) -> bool {
    let mut nodes = NODES.lock();
    let Some(n) = nodes.iter_mut().find(|n| n.hc == hc && n.slot == slot && n.kind == Kind::Keyboard) else { return false };
    let last = core::mem::replace(&mut n.last, *rep);
    if !raw {
        return false;
    }
    let mut keys = Vec::new();
    for (bit, &code) in MODIFIERS.iter().enumerate() {
        let (was, is) = (last[0] >> bit & 1, rep[0] >> bit & 1);
        if was != is {
            keys.push((code, is == 1));
        }
    }
    // a report with every key slot set to 1 is a rollover error: no change
    if rep[2..8].iter().all(|&u| u == 1) {
        return true;
    }
    for &u in last[2..8].iter().filter(|&&u| u > 3 && !rep[2..8].contains(&u)) {
        keys.push((hid_key(u), false));
    }
    for &u in rep[2..8].iter().filter(|&&u| u > 3 && !last[2..8].contains(&u)) {
        keys.push((hid_key(u), true));
    }
    for (code, down) in keys.into_iter().filter(|k| k.0 != 0) {
        push(n, 1, code, down as i32);
        push(n, 0, 0, 0);
    }
    crate::socket::bump();
    true
}

fn hid_key(u: u8) -> u16 {
    HID_KEYS.get(u as usize).map_or(0, |&k| k as u16)
}

/// One `struct input_event` (time of now) at the end of every reader's queue of node `n`.
fn push(n: &mut Node, typ: u16, code: u16, value: i32) {
    let e = record(typ, code, value);
    for (_, q) in n.queues.iter_mut() {
        if q.len() >= QUEUE || crate::heap::kernel(|| q.try_reserve(1)).is_err() {
            continue;
        }
        q.push_back(e);
    }
}

/// Reader `reader` (an open descriptor) starts reading node `index`: it gets its own queue and
/// from now on every event of the node. `false`: no such node, or no memory.
pub fn attach(index: u8, reader: u32) -> bool {
    let mut nodes = NODES.lock();
    let Some(n) = nodes.iter_mut().find(|n| n.index == index) else { return false };
    if n.queues.iter().any(|(r, _)| *r == reader) {
        return true;
    }
    if !crate::heap::kernel(|| crate::mem::try_reserve(&mut n.queues, 1)) {
        return false;
    }
    n.queues.push((reader, VecDeque::new()));
    true
}

/// Reader `reader` closed its descriptor: its queue goes.
pub fn detach(index: u8, reader: u32) {
    if let Some(n) = NODES.lock().iter_mut().find(|n| n.index == index) {
        n.queues.retain(|(r, _)| *r != reader);
    }
}

/// A `struct input_event` stamped with the time of now.
pub fn record(typ: u16, code: u16, value: i32) -> [u8; 24] {
    let us = crate::arch::uptime_ms() * 1000;
    let mut e = [0u8; 24];
    e[0..8].copy_from_slice(&(us / 1_000_000).to_le_bytes());
    e[8..16].copy_from_slice(&(us % 1_000_000).to_le_bytes());
    e[16..18].copy_from_slice(&typ.to_le_bytes());
    e[18..20].copy_from_slice(&code.to_le_bytes());
    e[20..24].copy_from_slice(&value.to_le_bytes());
    e
}

/// Numbers of the USB keyboard nodes, ascending.
pub fn usb_nodes() -> Vec<u8> {
    let mut v: Vec<u8> = NODES.lock().iter().map(|n| n.index).collect();
    v.sort_unstable();
    v
}

/// Whether node `index` exists (`event0`/`event1` exist with a screen).
pub fn exists(index: u8) -> bool {
    if index < FIRST {
        return crate::console::framebuffer().is_some();
    }
    NODES.lock().iter().any(|n| n.index == index)
}

/// Drop what the USB nodes hold (the first program opened an input node).
pub fn clear() {
    NODES.lock().iter_mut().for_each(|n| n.queues.iter_mut().for_each(|(_, q)| q.clear()));
}

/// Read whole events of reader `reader`, at most `max` bytes; `Ok(None)`: nothing there;
/// `ENODEV`: the device left (or the number belongs to a later device the reader never opened).
pub fn read(index: u8, reader: u32, max: usize) -> Result<Option<Vec<u8>>, i64> {
    crate::eingabe::pumpen();
    let mut nodes = NODES.lock();
    let Some(n) = nodes.iter_mut().find(|n| n.index == index) else { return Err(-ENODEV) };
    let Some((_, q)) = n.queues.iter_mut().find(|(r, _)| *r == reader) else { return Err(-ENODEV) };
    if q.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::new();
    while out.len() + 24 <= max {
        let Some(e) = q.pop_front() else { break };
        out.extend_from_slice(&e);
    }
    Ok(Some(out))
}

/// Whether a read of reader `reader` would not wait (events there, or the device left).
pub fn ready(index: u8, reader: u32) -> bool {
    crate::eingabe::pumpen();
    NODES
        .lock()
        .iter()
        .find(|n| n.index == index)
        .and_then(|n| n.queues.iter().find(|(r, _)| *r == reader))
        .map_or(true, |(_, q)| !q.is_empty())
}

/// Events written to a USB node (remote control): queued like the keyboard's own.
pub fn inject(index: u8, data: &[u8]) {
    let mut nodes = NODES.lock();
    let Some(n) = nodes.iter_mut().find(|n| n.index == index) else { return };
    for e in data.chunks_exact(24) {
        let (typ, code) = (u16::from_le_bytes([e[16], e[17]]), u16::from_le_bytes([e[18], e[19]]));
        push(n, typ, code, i32::from_le_bytes([e[20], e[21], e[22], e[23]]));
    }
    crate::socket::bump();
}

/// Name of node `index` (`EVIOCGNAME`, sysfs `name`).
pub fn name(index: u8) -> Option<String> {
    match index {
        0 => Some(String::from("Ultar Maus")),
        1 => Some(String::from("Ultar Tastatur")),
        _ => NODES.lock().iter().find(|n| n.index == index).map(|n| match n.caps.first() {
            Some(c) if !c.name.is_empty() => c.name.clone(),
            Some(_) => String::from("Ultar Virtual"),
            None => format!("USB {} {:04x}:{:04x}", word(n.kind), n.vendor, n.product),
        }),
    }
}

/// `struct input_id` of node `index`: bus, vendor, product, version.
fn id(index: u8) -> Option<[u16; 4]> {
    match index {
        0 | 1 => Some([BUS_I8042, 1, 1 + index as u16, 0]),
        // the bus of a virtual device is the kernel's, not the service's (S-4)
        _ => NODES.lock().iter().find(|n| n.index == index).map(|n| n.caps.first().map_or([BUS_USB, n.vendor, n.product, 0x0110], |c| [BUS_VIRTUAL, c.id[1], c.id[2], c.id[3]])),
    }
}

/// Bitmap with `bits` set, `bytes` long.
fn bitmap(bits: impl Iterator<Item = u16>, bytes: usize) -> Vec<u8> {
    let mut v = alloc::vec![0u8; bytes];
    for b in bits {
        if let Some(x) = v.get_mut(b as usize / 8) {
            *x |= 1 << (b % 8);
        }
    }
    v
}

/// Capabilities of node `index` for `EVIOCGBIT(ev)`: event types (ev 0), keys, relative and absolute
/// axes. The mouse reports buttons, the wheel and absolute X/Y; keyboards the key codes 1..=127.
fn bits(index: u8, ev: u8) -> Vec<u8> {
    // the PS/2 mouse (event0) reports absolute pixels like a tablet
    let kind = match index {
        0 => Kind::Tablet([0; 4]),
        1 => Kind::Keyboard,
        _ => {
            let nodes = NODES.lock();
            let n = nodes.iter().find(|n| n.index == index);
            if let Some(c) = n.and_then(|n| n.caps.first()) {
                return c.bits(ev);
            }
            n.map_or(Kind::Keyboard, |n| n.kind)
        }
    };
    let (pointer, abs) = (kind != Kind::Keyboard, matches!(kind, Kind::Tablet(_)));
    match ev {
        0 if abs => bitmap([0u16, 1, 2, 3].into_iter(), 4),
        0 if pointer => bitmap([0u16, 1, 2].into_iter(), 4),
        0 => bitmap([0u16, 1].into_iter(), 4),
        1 if pointer => bitmap([0x110u16, 0x111, 0x112].into_iter(), 96),
        1 => bitmap(1..=127u16, 96),
        2 if abs => bitmap([8u16].into_iter(), 2),
        2 if pointer => bitmap([0u16, 1, 8].into_iter(), 2),
        3 if abs => bitmap([0u16, 1].into_iter(), 8),
        _ => alloc::vec![0u8; 8],
    }
}

/// `EVIOCGABS(ABS_X/ABS_Y)` of a USB tablet: `struct input_absinfo`.
fn absinfo(index: u8, axis: u8) -> Option<Vec<u8>> {
    let nodes = NODES.lock();
    let n = nodes.iter().find(|n| n.index == index)?;
    if let Some(c) = n.caps.first() {
        return c.absinfo(axis);
    }
    let Kind::Tablet(r) = n.kind else { return None };
    if axis > 1 {
        return None;
    }
    let (min, max) = if axis == 0 { (r[0], r[1]) } else { (r[2], r[3]) };
    Some([0, min, max, 0, 0, 0].iter().flat_map(|v: &i32| v.to_le_bytes()).collect())
}

/// A report of the USB mouse or tablet in `slot`: buttons (bit 0 left, 1 right, 2 middle), X and Y
/// (relative motion or absolute position), wheel. Queued while a program reads input nodes.
pub fn usb_pointer(hc: usize, slot: u32, buttons: u8, x: i32, y: i32, wheel: i32) {
    let raw = crate::eingabe::roh();
    let mut nodes = NODES.lock();
    let Some(n) = nodes.iter_mut().find(|n| n.hc == hc && n.slot == slot && n.kind != Kind::Keyboard) else { return };
    let old = core::mem::replace(&mut n.last[0], buttons);
    if !raw {
        return;
    }
    match n.kind {
        Kind::Tablet(_) => {
            push(n, 3, 0, x);
            push(n, 3, 1, y);
        }
        _ => {
            if x != 0 {
                push(n, 2, 0, x);
            }
            if y != 0 {
                push(n, 2, 1, y);
            }
        }
    }
    for (bit, code) in [(1u8, 0x110u16), (2, 0x111), (4, 0x112)] {
        if (buttons ^ old) & bit != 0 {
            push(n, 1, code, (buttons & bit != 0) as i32);
        }
    }
    if wheel != 0 {
        push(n, 2, 8, wheel);
    }
    push(n, 0, 0, 0);
    crate::socket::bump();
}

/// The `ioctl`s every node answers: `EVIOCGVERSION`, `EVIOCGID`, `EVIOCGNAME`, `EVIOCGPHYS`,
/// `EVIOCGUNIQ`, `EVIOCGPROP`, `EVIOCGKEY`/`LED`/`SND`/`SW`, `EVIOCGBIT`, `EVIOCGRAB`. `None`: unknown.
pub fn ioctl(index: u8, req: u64) -> Option<Vec<u8>> {
    if (req >> 8) & 0xff != 0x45 {
        return None;
    }
    let nr = (req & 0xff) as u8;
    match (req >> 30, nr) {
        (2, 0x01) => Some(0x0001_0001u32.to_le_bytes().to_vec()),
        (2, 0x02) => Some(id(index)?.iter().flat_map(|x| x.to_le_bytes()).collect()),
        (2, 0x06) => name(index).map(|n| [n.as_bytes(), b"\0"].concat()),
        (2, 0x07) => Some(format!("ultar/input{index}\0").into_bytes()),
        (2, 0x08) => Some(alloc::vec![0u8]),
        (2, 0x09 | 0x18 | 0x19 | 0x1a | 0x1b) => Some(alloc::vec![0u8; 8]),
        (2, 0x20..=0x3f) => Some(bits(index, nr - 0x20)),
        (2, 0x40..=0x7f) => absinfo(index, nr - 0x40),
        (1, 0x90) => Some(Vec::new()),
        _ => None,
    }
}

/// Whether `EVIOCG*` request `req` returns the number of bytes it wrote (variable-length requests).
pub fn returns_len(req: u64) -> bool {
    (req >> 8) & 0xff == 0x45 && req >> 30 == 2 && matches!(req & 0xff, 0x06..=0x09 | 0x18..=0x1b | 0x20..=0x3f)
}

/// `dev_t` of node `index` (major 13, minor 64 + index).
pub fn rdev(index: u8) -> u64 {
    13 << 8 | (64 + index as u64)
}

/// Announce node `index` (`add`, `remove`, `change`).
pub fn uevent(index: u8, action: &str) {
    let vars = [
        ("DEVNAME", format!("input/event{index}")),
        ("MAJOR", String::from("13")),
        ("MINOR", format!("{}", 64 + index as u32)),
        ("NAME", format!("\"{}\"", name(index).unwrap_or_default())),
    ];
    crate::devices::events::emit(action, &format!("/devices/ultar/input/input{index}/event{index}"), "input", &vars);
}

/// Every node that exists: the fixed ones (with a screen) and the USB keyboards.
pub fn all() -> Vec<u8> {
    let mut v: Vec<u8> = if crate::console::framebuffer().is_some() { alloc::vec![0, 1] } else { Vec::new() };
    v.extend(usb_nodes());
    v
}

/// Number of `eventN` (`None` for another name).
pub fn parse(name: &str) -> Option<u8> {
    let n = name.strip_prefix("event")?;
    (!n.is_empty() && n.len() <= 3 && n.bytes().all(|b| b.is_ascii_digit())).then(|| n.parse().ok()).flatten()
}

/// Text of a file below `/sys/class/input/eventN` (`rest`: the path below that).
pub fn sys_file(index: u8, rest: &[&str]) -> Option<String> {
    if !exists(index) {
        return None;
    }
    match rest {
        ["dev"] => Some(format!("13:{}\n", 64 + index as u32)),
        ["uevent"] => Some(format!("MAJOR=13\nMINOR={}\nDEVNAME=input/event{index}\n", 64 + index as u32)),
        ["device", "name"] => name(index).map(|n| n + "\n"),
        // as Linux: `device/id/{bustype,vendor,product,version}`, four hex digits
        ["device", "id", field] => {
            let i = id(index)?;
            let at = ["bustype", "vendor", "product", "version"].iter().position(|f| f == field)?;
            Some(format!("{:04x}\n", i[at]))
        }
        ["device", "uevent"] => {
            let i = id(index)?;
            Some(format!("PRODUCT={:x}/{:x}/{:x}/{:x}\nNAME=\"{}\"\n", i[0], i[1], i[2], i[3], name(index)?))
        }
        _ => None,
    }
}
