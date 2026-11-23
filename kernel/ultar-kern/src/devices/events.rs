//! Device events (uevents) in the Linux format, delivered on `AF_NETLINK` sockets of protocol
//! `NETLINK_KOBJECT_UEVENT` and kept in a short history.
//!
//! One message: `ACTION@DEVPATH\0ACTION=…\0DEVPATH=…\0SUBSYSTEM=…\0SEQNUM=n\0…` – every key ends
//! with a NUL, as from Linux. A read returns exactly one message (a longer one is cut, the rest
//! dropped, like a datagram). `SEQNUM` counts every event of the machine from 1 without gaps; a
//! reader that sees a gap lost events (its queue was full) and reads the device tree again.
//! `/sys/kernel/uevent_seqnum` is the last number given out.

use crate::pipe::Pipe;
use crate::proc::{Fd, FdKind};
use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

/// `AF_NETLINK`.
pub const AF_NETLINK: u64 = 16;
/// `NETLINK_KOBJECT_UEVENT`.
pub const NETLINK_KOBJECT_UEVENT: u64 = 15;
/// Queue of one listening socket (a few hundred events).
const QUEUE: u64 = 64 * 1024;
/// Most listening sockets at once.
const MAX_LISTENERS: usize = 64;
/// Events kept for `/sys/kernel/ultar/uevents`.
const HISTORY: usize = 128;

static SEQNUM: AtomicU64 = AtomicU64::new(0);
static LISTENERS: Mutex<Vec<Weak<Pipe>>> = Mutex::new(Vec::new());
static RECENT: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
/// Messages that did not fit into a listener's queue.
static LOST: AtomicU64 = AtomicU64::new(0);

/// The last sequence number given out.
pub fn seqnum() -> u64 {
    SEQNUM.load(Ordering::SeqCst)
}

/// Messages that were dropped because a listener's queue was full.
pub fn lost() -> u64 {
    LOST.load(Ordering::Relaxed)
}

/// The recent events, one per line: `SEQNUM ACTION DEVPATH SUBSYSTEM KEY=VALUE…`.
pub fn recent() -> String {
    let mut s = String::new();
    for e in RECENT.lock().iter() {
        s += e;
        s.push('\n');
    }
    s
}

/// Send one event to every listener. `vars`: further `KEY=VALUE` pairs.
pub fn emit(action: &str, devpath: &str, subsystem: &str, vars: &[(&str, String)]) {
    crate::heap::kernel(|| emit_inner(action, devpath, subsystem, vars));
    // wake readers waiting in read/poll/epoll
    crate::socket::bump();
}

fn emit_inner(action: &str, devpath: &str, subsystem: &str, vars: &[(&str, String)]) {
    // numbers are given out under the list's lock: listeners see them in order
    let mut list = LISTENERS.lock();
    let n = SEQNUM.fetch_add(1, Ordering::SeqCst) + 1;
    let mut msg = format!("{action}@{devpath}\0ACTION={action}\0DEVPATH={devpath}\0SUBSYSTEM={subsystem}\0SEQNUM={n}\0");
    let mut line = format!("{n} {action} {devpath} {subsystem}");
    for (k, v) in vars {
        msg += &format!("{k}={v}\0");
        line += &format!(" {k}={v}");
    }
    list.retain(|w| w.upgrade().is_some_and(|p| p.readers.load(Ordering::SeqCst) > 0));
    for p in list.iter().filter_map(Weak::upgrade) {
        if !p.push_packet(msg.as_bytes()) {
            LOST.fetch_add(1, Ordering::Relaxed);
        }
    }
    let mut r = RECENT.lock();
    if r.len() == HISTORY {
        r.pop_front();
    }
    r.push_back(line);
}

/// Event for the PCI function at `DEVICES[i]` (`bind`, `unbind`, `change`).
pub fn pci(action: &str, i: usize) {
    let vars = {
        let devs = crate::driver::DEVICES.lock();
        let Some(b) = devs.get(i) else { return };
        let mut v = alloc::vec![
            ("PCI_SLOT_NAME", super::pci_name(&b.dev)),
            ("PCI_ID", format!("{:04X}:{:04X}", b.dev.vendor, b.dev.device)),
            ("PCI_CLASS", format!("{:X}", (b.dev.class as u32) << 16 | (b.dev.subclass as u32) << 8 | b.dev.prog_if as u32)),
            ("ULTAR_STATE", String::from(super::state(b))),
        ];
        if let Some(d) = b.driver {
            v.push(("DRIVER", String::from(d)));
        }
        v
    };
    let path = format!("/bus/pci/devices/{}", vars[0].1);
    emit(action, &path, "pci", &vars);
}

/// `socket(AF_NETLINK, SOCK_RAW|SOCK_DGRAM, NETLINK_KOBJECT_UEVENT)`: a descriptor that reads the
/// events from now on. Only for processes of the machine's own pid namespace (not cells).
pub fn socket(typ: u64, proto: u64) -> i64 {
    if !matches!(typ & 0xf, 2 | 3) || proto != NETLINK_KOBJECT_UEVENT {
        return -93;
    }
    if !crate::proc::reader_in_initial_pid_ns() {
        return -13;
    }
    let p = crate::heap::kernel(|| Pipe::packets(QUEUE));
    // the kernel is the writer: the queue never reports end of file
    p.readers.store(1, Ordering::SeqCst);
    p.writers.store(1, Ordering::SeqCst);
    {
        let mut list = LISTENERS.lock();
        list.retain(|w| w.upgrade().is_some_and(|p| p.readers.load(Ordering::SeqCst) > 0));
        if list.len() >= MAX_LISTENERS {
            return -105;
        }
        crate::heap::kernel(|| list.push(Arc::downgrade(&p)));
    }
    let flags = 2 | (typ as u32 & 0x800);
    crate::syscall::with(|pr| pr.add_fd(Fd { kind: FdKind::PipeR(p), cloexec: typ & 0x80000 != 0, flags }, 0)).unwrap_or(-9)
}

/// Whether `fd` of the calling process is an event socket (`bind` and `setsockopt` succeed on it).
pub fn is_event_fd(fd: u64) -> bool {
    let Some(Some(p)) = crate::syscall::with(|pr| match pr.fd(fd).map(|d| &d.kind) {
        Some(FdKind::PipeR(p)) => Some(p.clone()),
        _ => None,
    }) else {
        return false;
    };
    LISTENERS.lock().iter().any(|w| core::ptr::eq(w.as_ptr(), Arc::as_ptr(&p)))
}
