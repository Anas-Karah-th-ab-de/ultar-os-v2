//! `/dev/dri/renderD128`: the GPU for programs, with the requests of the Linux `xe` driver, so
//! that Mesa from the Ubuntu packages can talk to it unchanged (stage G4 of
//! `docs/integration/intel-gt.md`). The request logic is `ultar_intel_gt::uapi`; this module
//! is the file: it exists once the GT driver published what the queries answer from, it moves
//! request structures between the program and that logic, and it keeps what each open owns
//! (buffers, address spaces, queues – ended when the last descriptor closes).
//!
//! Questions (version, capabilities, device queries) are always answered; everything else
//! needs the GT in operation (`ENODEV` otherwise).
//!
//! Gate: [`RIGHT`] (raw device access), default deny. Which programs get the GPU later is a
//! separate decision; until then only privileged services reach the node.

use super::{EFAULT, ENODEV, ENOTTY};
use crate::drivers::intel_display::gt_exec::Gpu;
use crate::isolation::rights::Rights;
use crate::memfd::MemObj;
use crate::proc::{self, FdKind};
use crate::syscall::with;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;
use ultar_intel_gt::uapi::file::File;
use ultar_intel_gt::uapi::{self, drm, Errno};

/// Name of the node below `/dev/`.
pub const NODE: &str = "dri/renderD128";
/// The right that opens the node.
pub const RIGHT: Rights = Rights::RAW_DEVICE;
/// What `ioctl` returns inside the kernel when the program has to sleep a moment and ask
/// again (a wait that is not over); never seen by a program.
pub const RETRY: i64 = i64::MIN;

static DEVICE: Mutex<Option<Arc<uapi::Device>>> = Mutex::new(None);

/// Makes the node appear: `device` is what the queries answer from.
pub fn publish(device: uapi::Device) {
    *DEVICE.lock() = Some(Arc::new(device));
}

/// Whether the node exists.
pub fn present() -> bool {
    DEVICE.lock().is_some()
}

/// An open render node: what the opener created.
pub struct DriFd(Mutex<File<Gpu>>);

impl Drop for DriFd {
    fn drop(&mut self) {
        // the last descriptor is gone: queues and address spaces end with it
        if let Some(mut gpu) = Gpu::lock() {
            self.0.lock().close(&mut gpu);
        }
    }
}

/// Opens the node.
pub fn open() -> Result<FdKind, i64> {
    if !present() {
        return Err(-ENODEV);
    }
    Ok(FdKind::Dri(Arc::new(DriFd(Mutex::new(File::default())))))
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

/// One request of the open `fd` against the published device: `arg` is the request's
/// structure. Returns 0, a negative error number or [`RETRY`]. Also the entry for the
/// kernel's self-test.
pub fn request(fd: &DriFd, req: u32, arg: &mut [u8], user: &mut impl drm::User) -> i64 {
    let Some(device) = DEVICE.lock().clone() else {
        return -ENODEV;
    };
    let mut gpu = Gpu::lock();
    let mut file = fd.0.lock();
    match drm::ioctl(&device, &mut file, gpu.as_mut(), req, arg, user) {
        Ok(()) => 0,
        Err(Errno::Again) => RETRY,
        Err(e) => -(e as i64),
    }
}

/// A file of the kernel's own, for the self-test.
pub fn selftest_file() -> DriFd {
    DriFd(Mutex::new(File::default()))
}

fn handle(fd: u64) -> Option<Arc<DriFd>> {
    with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(FdKind::Dri(d)) => Some(d.clone()),
        _ => None,
    })
    .flatten()
}

/// `ioctl` on the node; `None` if `fd` is not one.
pub fn sys_ioctl(fd: u64, req: u64, arg: u64) -> Option<i64> {
    let node = handle(fd)?;
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
    let ret = request(&node, req as u32, &mut buf, &mut user);
    if ret == 0 && r.read && !buf.is_empty() && !drm::User::write(&mut user, arg, &buf) {
        return Some(-EFAULT);
    }
    Some(ret)
}

/// The buffer a program maps with `mmap(fd, offset)`: the offset comes from the mmap-offset
/// request. `None`: no such buffer, or `len` reaches beyond it.
pub fn mmap_object(fd: &DriFd, offset: u64, len: u64) -> Option<Arc<MemObj>> {
    let file = fd.0.lock();
    let (buffer, size) = file.buffer_at(offset)?;
    (len <= size).then(|| buffer.clone())
}
