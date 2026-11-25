//! `/dev/dri/renderD128`: the GPU for programs, with the requests of the Linux `xe` driver, so
//! that Mesa from the Ubuntu packages can talk to it unchanged (stage G4 of
//! `docs/integration/intel-gt.md`). The request logic is `ultar_intel_gt::uapi`; this module
//! is the file: it exists once the GT driver published what the queries answer from, and it
//! moves request structures between the program and that logic.
//!
//! Stage G4b: version, capabilities, device queries. Requests for buffers, address spaces and
//! jobs are refused (`EINVAL`) until they exist.
//!
//! Gate: [`RIGHT`] (raw device access), default deny. Which programs get the GPU later is a
//! separate decision; until then only privileged services reach the node.

use super::{EFAULT, ENODEV, ENOTTY};
use crate::isolation::rights::Rights;
use crate::proc::{self, FdKind};
use crate::syscall::with;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;
use ultar_intel_gt::uapi::{self, drm, Errno};

/// Name of the node below `/dev/`.
pub const NODE: &str = "dri/renderD128";
/// The right that opens the node.
pub const RIGHT: Rights = Rights::RAW_DEVICE;

static DEVICE: Mutex<Option<uapi::Device>> = Mutex::new(None);

/// Makes the node appear: `device` is what the queries answer from.
pub fn publish(device: uapi::Device) {
    *DEVICE.lock() = Some(device);
}

/// Whether the node exists.
pub fn present() -> bool {
    DEVICE.lock().is_some()
}

/// An open render node. Buffers, address spaces and queues of the opener will hang here.
pub struct DriFd;

/// Opens the node.
pub fn open() -> Result<FdKind, i64> {
    if !present() {
        return Err(-ENODEV);
    }
    Ok(FdKind::Dri(Arc::new(DriFd)))
}

/// The calling process' memory.
struct Caller;

impl drm::User for Caller {
    fn write(&mut self, addr: u64, data: &[u8]) -> bool {
        with(|p| proc::write_user(p.space(), addr, data)).unwrap_or(false)
    }
    fn read(&mut self, addr: u64, len: usize) -> Option<Vec<u8>> {
        with(|p| proc::read_user(p.space(), addr, len)).flatten()
    }
}

fn errno(e: Errno) -> i64 {
    -(e as i64)
}

/// One request against the published device: `arg` is the request's structure. Returns 0 or a
/// negative error number. Also the entry for the kernel's self-test.
pub fn request(req: u32, arg: &mut [u8], user: &mut impl drm::User) -> i64 {
    let guard = DEVICE.lock();
    let Some(device) = guard.as_ref() else {
        return -ENODEV;
    };
    match drm::ioctl(device, req, arg, user) {
        Ok(()) => 0,
        Err(e) => errno(e),
    }
}

/// `ioctl` on the node; `None` if `fd` is not one.
pub fn sys_ioctl(fd: u64, req: u64, arg: u64) -> Option<i64> {
    with(|p| matches!(p.fd(fd).map(|d| &d.kind), Some(FdKind::Dri(_)))).filter(|is| *is)?;
    let Some(r) = u32::try_from(req).ok().and_then(uapi::Request::decode) else {
        return Some(-ENOTTY);
    };
    let mut user = Caller;
    let mut buf = match r.size {
        0 => Vec::new(),
        n => match drm::User::read(&mut user, arg, n as usize) {
            Some(b) => b,
            None => return Some(-EFAULT),
        },
    };
    let ret = request(req as u32, &mut buf, &mut user);
    if ret == 0 && r.read && !buf.is_empty() && !drm::User::write(&mut user, arg, &buf) {
        return Some(-EFAULT);
    }
    Some(ret)
}
