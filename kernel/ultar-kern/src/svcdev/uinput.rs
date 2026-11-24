//! `/dev/ultar/uinput`: virtual input devices for privileged services (the Bluetooth service's
//! keyboards and mice, remote control), with Linux' `uinput` interface so the same code serves
//! both kernels.
//!
//! * open: right `INPUT_INJECT` only; each descriptor creates at most one device;
//! * declare what the device sends: `UI_SET_EVBIT` (keys, relative and absolute axes),
//!   `UI_SET_KEYBIT` / `UI_SET_RELBIT` / `UI_SET_ABSBIT` (integer argument), name and id by
//!   `UI_DEV_SETUP` or by writing the legacy `struct uinput_user_dev` (which also carries the
//!   ranges of absolute axes; `UI_ABS_SETUP` sets one range);
//! * `UI_DEV_CREATE`: the device appears as a normal `/dev/input/eventN` (device event, sysfs,
//!   `EVIOCG*` answer what was declared); `UI_GET_SYSNAME` names it `inputN` (N = event number);
//! * `write` of whole `struct input_event` records: each must be declared (else `EINVAL`, nothing
//!   delivered; absolute values are held to their range); at most [`MAX_EVENTS_PER_WRITE`] per
//!   call; rate limit: [`RATE`] events per second with bursts of [`BURST`] – beyond that `EAGAIN`;
//! * `UI_DEV_DESTROY` or the last close removes the device (keys still held are gone with it).

use super::{EAGAIN, EFAULT, EINVAL, ENOMEM, ENOSPC};
use crate::input::virt::{Caps, EV_ABS, EV_KEY, EV_REL};
use crate::proc::{self, FdKind};
use crate::syscall::with;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

/// Events per second a device may send in the long run.
pub const RATE: u64 = 1000;
/// Events a device may send at once.
pub const BURST: u64 = 256;
/// Most events in one `write`.
pub const MAX_EVENTS_PER_WRITE: usize = 256;
/// `sizeof(struct uinput_user_dev)`.
const USER_DEV: usize = 80 + 8 + 4 + 4 * 64 * 4;
/// `sizeof(struct input_event)` (64-bit).
const EVENT: usize = 24;
/// Interface version (`UI_GET_VERSION`).
const VERSION: u32 = 5;

const UI_DEV_CREATE: u64 = 0x5501;
const UI_DEV_DESTROY: u64 = 0x5502;
const UI_DEV_SETUP: u64 = 0x405c_5503;
const UI_ABS_SETUP: u64 = 0x401c_5504;
const UI_SET_EVBIT: u64 = 0x4004_5564;
const UI_SET_KEYBIT: u64 = 0x4004_5565;
const UI_SET_RELBIT: u64 = 0x4004_5566;
const UI_SET_ABSBIT: u64 = 0x4004_5567;
const UI_GET_VERSION: u64 = 0x8004_552d;
/// `UI_GET_SYSNAME(len)`: number 44, read, any length.
const UI_GET_SYSNAME_NR: u64 = 0x8000_552c;

/// State of one descriptor.
struct State {
    caps: Caps,
    /// Node number of the created device.
    node: Option<u8>,
    /// Rate limit: events that may still be sent (×1000), and when it was last filled up.
    tokens: u64,
    filled_ms: u64,
}

/// An open `uinput` node.
pub struct UinputFd(Mutex<State>);

impl Drop for UinputFd {
    fn drop(&mut self) {
        if let Some(n) = self.0.lock().node.take() {
            crate::input::virtual_removed(n);
        }
    }
}

/// A new descriptor (the caller holds the right).
pub fn open() -> FdKind {
    FdKind::Uinput(Arc::new(UinputFd(Mutex::new(State {
        caps: Caps::default(),
        node: None,
        tokens: BURST * 1000,
        filled_ms: crate::arch::uptime_ms(),
    }))))
}

fn handle(fd: u64) -> Option<Arc<UinputFd>> {
    with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(FdKind::Uinput(u)) => Some(u.clone()),
        _ => None,
    })
    .flatten()
}

fn user(arg: u64, len: usize) -> Option<Vec<u8>> {
    with(|p| proc::read_user(p.space(), arg, len)).flatten()
}

fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

/// `ioctl` on the node; `None` if `fd` is not one.
pub fn sys_ioctl(fd: u64, req: u64, arg: u64) -> Option<i64> {
    let u = handle(fd)?;
    let mut st = u.0.lock();
    let r = match req {
        UI_GET_VERSION => {
            return Some(
                if with(|p| proc::write_user(p.space(), arg, &VERSION.to_le_bytes()))
                    .unwrap_or(false)
                {
                    0
                } else {
                    -EFAULT
                },
            )
        }
        r if r & 0xc000_ffff == UI_GET_SYSNAME_NR => {
            let Some(n) = st.node else {
                return Some(-EINVAL);
            };
            let name = alloc::format!("input{n}\0");
            let len = ((req >> 16) & 0x3fff) as usize;
            if len < name.len() {
                return Some(-EINVAL);
            }
            return Some(
                if with(|p| proc::write_user(p.space(), arg, name.as_bytes())).unwrap_or(false) {
                    name.len() as i64
                } else {
                    -EFAULT
                },
            );
        }
        UI_DEV_DESTROY => {
            if let Some(n) = st.node.take() {
                crate::input::virtual_removed(n);
            }
            return Some(0);
        }
        _ if st.node.is_some() => return Some(-EINVAL),
        UI_SET_EVBIT => arg <= 0xffff && st.caps.set_ev(arg as u16),
        UI_SET_KEYBIT => arg <= 0xffff && st.caps.set_code(EV_KEY, arg as u16),
        UI_SET_RELBIT => arg <= 0xffff && st.caps.set_code(EV_REL, arg as u16),
        UI_SET_ABSBIT => arg <= 0xffff && st.caps.set_code(EV_ABS, arg as u16),
        UI_DEV_SETUP => {
            let Some(b) = user(arg, 92) else {
                return Some(-EFAULT);
            };
            st.caps.id = [u16_at(&b, 0), u16_at(&b, 2), u16_at(&b, 4), u16_at(&b, 6)];
            st.caps.set_name(&b[8..88]);
            true
        }
        UI_ABS_SETUP => {
            let Some(b) = user(arg, 28) else {
                return Some(-EFAULT);
            };
            st.caps
                .set_range(u16_at(&b, 0), i32_at(&b, 8), i32_at(&b, 12))
        }
        UI_DEV_CREATE => {
            if !st.caps.complete() {
                return Some(-EINVAL);
            }
            match crate::input::virtual_added(st.caps.clone()) {
                Some(n) => {
                    st.node = Some(n);
                    true
                }
                None => return Some(-ENOSPC),
            }
        }
        _ => return Some(-25), // ENOTTY
    };
    Some(if r { 0 } else { -EINVAL })
}

/// Fill the rate-limit bucket for the time that passed; whether `n` events may go now (and
/// take them).
fn take_tokens(st: &mut State, n: u64) -> bool {
    let now = crate::arch::uptime_ms();
    let add = now.saturating_sub(st.filled_ms) * RATE;
    st.tokens = (st.tokens + add).min(BURST * 1000);
    st.filled_ms = now;
    if st.tokens < n * 1000 {
        return false;
    }
    st.tokens -= n * 1000;
    true
}

/// The legacy setup record (before `UI_DEV_CREATE`): name, id, ranges of absolute axes.
fn user_dev(st: &mut State, b: &[u8]) -> bool {
    st.caps.set_name(&b[..80]);
    st.caps.id = [u16_at(b, 80), u16_at(b, 82), u16_at(b, 84), u16_at(b, 86)];
    (0..64u16).all(|a| {
        let (max, min) = (
            i32_at(b, 92 + a as usize * 4),
            i32_at(b, 92 + 256 + a as usize * 4),
        );
        (min == 0 && max == 0) || st.caps.set_range(a, min, max)
    })
}

/// `write` on the node; `None` if `fd` is not one.
pub fn sys_write(fd: u64, buf: u64, n: u64) -> Option<i64> {
    let u = handle(fd)?;
    let n = n as usize;
    let mut st = u.0.lock();
    let Some(node) = st.node else {
        if n != USER_DEV {
            return Some(-EINVAL);
        }
        let Some(b) = user(buf, n) else {
            return Some(-EFAULT);
        };
        return Some(if user_dev(&mut st, &b) {
            n as i64
        } else {
            -EINVAL
        });
    };
    if n == 0 || n % EVENT != 0 || n / EVENT > MAX_EVENTS_PER_WRITE {
        return Some(-EINVAL);
    }
    let Some(b) = user(buf, n) else {
        return Some(-EFAULT);
    };
    let mut events = Vec::new();
    if crate::heap::kernel(|| events.try_reserve(n / EVENT).is_err()) {
        return Some(-ENOMEM);
    }
    events.extend(
        b.chunks_exact(EVENT)
            .map(|e| (u16_at(e, 16), u16_at(e, 18), i32_at(e, 20))),
    );
    if !take_tokens(&mut st, events.len() as u64) {
        return Some(-EAGAIN);
    }
    Some(if crate::input::virtual_events(node, &events) {
        n as i64
    } else {
        -EINVAL
    })
}

/// `read` on the node: there is nothing to read (no force feedback).
pub fn sys_read(fd: u64) -> Option<i64> {
    handle(fd).map(|_| -EINVAL)
}
