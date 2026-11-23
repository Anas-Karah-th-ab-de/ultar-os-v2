//! Kernel device events (`NETLINK_KOBJECT_UEVENT` through
//! `ultar_sys::uevent`; Linux and the Ultar kernel use the same format): a
//! seat node that comes later (hot-plug) is handled when its event arrives.

use std::time::Duration;
use ultar_hal_system::seat_nodes::relative_devname;
use ultar_sys::uevent::{Uevent, UeventSocket};

/// Most events read in one batch.
const BATCH: usize = 256;
/// `ENOBUFS`: the receive buffer overflowed, events were lost.
const NOBUFS: i32 = 105;

/// What one event says about a device node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEvent {
    /// `add`, `change`, `remove`, …
    pub action: String,
    /// `DEVNAME=` (the node below `/dev`); empty if the event names none.
    pub devname: String,
}

impl DeviceEvent {
    /// The node part of a kernel event.
    pub fn of(event: &Uevent) -> DeviceEvent {
        let devname = event
            .var("DEVNAME")
            .map(relative_devname)
            .unwrap_or_default();
        DeviceEvent {
            action: event.action.clone(),
            devname: devname.to_owned(),
        }
    }

    /// Parses `ACTION@DEVPATH\0KEY=VALUE\0…`; `None` for anything else.
    pub fn parse(message: &[u8]) -> Option<DeviceEvent> {
        Uevent::parse(message).map(|event| Self::of(&event))
    }

    /// True if the node may be new or changed (and has to be set again).
    /// Only the name is taken from the event; owner and mode follow from sysfs.
    pub fn wants_apply(&self) -> bool {
        matches!(self.action.as_str(), "add" | "change" | "bind")
            && super::is_seat_node(&self.devname)
    }
}

/// What a wait brought.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Batch {
    /// Events in arrival order.
    pub events: Vec<DeviceEvent>,
    /// Events were lost or the socket failed: everything has to be scanned.
    pub lost: bool,
}

/// A subscription to the kernel's device events.
#[derive(Debug)]
pub struct EventSocket {
    socket: UeventSocket,
}

impl EventSocket {
    /// Subscribes; fails where the kernel has no device events.
    pub fn open() -> std::io::Result<EventSocket> {
        UeventSocket::open()
            .map(|socket| EventSocket { socket })
            .map_err(|errno| std::io::Error::from_raw_os_error(errno.0))
    }

    /// Waits up to `timeout` for the first event, then takes what else is
    /// pending (at most [`BATCH`]). A failed socket is reported as lost events
    /// after waiting `timeout`, so the caller never spins.
    pub fn wait(&self, timeout: Duration) -> Batch {
        let mut batch = Batch::default();
        let mut wait = Some(timeout);
        while batch.events.len() < BATCH {
            match self.socket.next(wait) {
                Ok(Some(event)) => batch.events.push(DeviceEvent::of(&event)),
                Ok(None) => break,
                Err(errno) => {
                    if errno.0 != NOBUFS {
                        std::thread::sleep(timeout);
                    }
                    batch.lost = true;
                    break;
                }
            }
            wait = Some(Duration::ZERO);
        }
        batch
    }
}
