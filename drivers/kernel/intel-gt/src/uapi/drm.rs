//! The requests every DRM driver answers (version, capabilities) and the entry point that
//! takes any request on the device file. Layouts: `include/uapi/drm/drm.h` (MIT-style licence).

use super::file::{Backend, File};
use super::{Answer, Device, DeviceQuery, Errno, Request};
use alloc::vec::Vec;

/// Request: driver name and version.
pub const DRM_VERSION: u32 = 0x00;
/// Request: one capability.
pub const DRM_GET_CAP: u32 = 0x0c;
/// Request: give up a buffer handle.
pub const DRM_GEM_CLOSE: u32 = 0x09;

/// Capability: time stamps are monotonic.
pub const CAP_TIMESTAMP_MONOTONIC: u64 = 0x6;
/// Capability: sync objects.
pub const CAP_SYNCOBJ: u64 = 0x13;
/// Capability: sync objects with timelines.
pub const CAP_SYNCOBJ_TIMELINE: u64 = 0x14;
/// The highest capability number DRM knows.
const CAP_LAST: u64 = 0x15;

/// Name programs select their driver by.
pub const DRIVER_NAME: &str = "xe";
/// Description of the driver.
pub const DRIVER_DESC: &str = "Intel Xe Graphics (Ultar kernel)";
/// Version of the interface this kernel follows.
pub const DRIVER_VERSION: (i32, i32, i32) = (1, 1, 0);

/// Memory of the calling program.
pub trait User {
    /// Copies `data` to the program's address `addr`; `false` if that is not its memory.
    fn write(&mut self, addr: u64, data: &[u8]) -> bool;
    /// Reads `len` bytes at the program's address `addr`.
    fn read(&mut self, addr: u64, len: usize) -> Option<Vec<u8>>;
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    let mut w = [0u8; 8];
    w.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(w)
}

fn put64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

/// `struct drm_version` (64 bytes): three numbers, then length and address for name, date and
/// description. Each text is copied as far as the program's buffer reaches, and its full
/// length is written back.
fn version(arg: &mut [u8], user: &mut impl User) -> Result<(), Errno> {
    if arg.len() < 64 {
        return Err(Errno::Inval);
    }
    let (major, minor, patch) = DRIVER_VERSION;
    arg[0..4].copy_from_slice(&major.to_le_bytes());
    arg[4..8].copy_from_slice(&minor.to_le_bytes());
    arg[8..12].copy_from_slice(&patch.to_le_bytes());
    for (at, text) in [(16, DRIVER_NAME), (32, ""), (48, DRIVER_DESC)] {
        let (len, addr) = (u64_at(arg, at) as usize, u64_at(arg, at + 8));
        let n = len.min(text.len());
        if n > 0 && addr != 0 && !user.write(addr, &text.as_bytes()[..n]) {
            return Err(Errno::Fault);
        }
        put64(arg, at, text.len() as u64);
    }
    Ok(())
}

/// `struct drm_get_cap` (capability, value). Only what the kernel really does is announced.
fn get_cap(arg: &mut [u8]) -> Result<(), Errno> {
    if arg.len() < 16 {
        return Err(Errno::Inval);
    }
    let value = match u64_at(arg, 0) {
        CAP_TIMESTAMP_MONOTONIC | CAP_SYNCOBJ | CAP_SYNCOBJ_TIMELINE => 1,
        1..=CAP_LAST => 0,
        _ => return Err(Errno::Inval),
    };
    put64(arg, 8, value);
    Ok(())
}

fn device_query(dev: &Device, arg: &mut [u8], user: &mut impl User) -> Result<(), Errno> {
    let q = DeviceQuery::parse(arg)?;
    let input = match q.size {
        0 => Vec::new(),
        // only the firmware query reads the program's buffer, and that one is 32 bytes
        n => user
            .read(q.data, (n as usize).min(32))
            .ok_or(Errno::Fault)?,
    };
    match dev.query(q.query, q.size, &input)? {
        Answer::Size(size) => arg[12..16].copy_from_slice(&size.to_le_bytes()),
        Answer::Data(data) => {
            if !user.write(q.data, &data) {
                return Err(Errno::Fault);
            }
        }
    }
    Ok(())
}

/// Handles request `req` on the device file for the open `file`. `arg` is the request's
/// structure, copied from the program ([`Request::size`] bytes); the caller copies it back
/// when the request reads ([`Request::read`]) and this returns `Ok`. A number that is not DRM's
/// is refused with [`Errno::NoTty`], a DRM request the kernel does not have with
/// [`Errno::Inval`]. [`Errno::Again`] from a wait means: let the program sleep and ask again.
/// `gpu` is `None` while the GT is not in operation: then only questions are answered.
pub fn ioctl<B: Backend>(
    dev: &Device,
    file: &mut File<B>,
    gpu: Option<&mut B>,
    req: u32,
    arg: &mut [u8],
    user: &mut impl User,
) -> Result<(), Errno> {
    let r = Request::decode(req).ok_or(Errno::NoTty)?;
    if arg.len() < r.size as usize {
        return Err(Errno::Inval);
    }
    match (r.nr, r.driver()) {
        (DRM_VERSION, _) => return version(arg, user),
        (DRM_GET_CAP, _) => return get_cap(arg),
        (_, Some(super::XE_DEVICE_QUERY)) => return device_query(dev, arg, user),
        _ => {}
    }
    let be = gpu.ok_or(Errno::NoDev)?;
    file.reap(be, user);
    match (r.nr, r.driver()) {
        (DRM_GEM_CLOSE, _) => file.gem_close(arg),
        (0xbf, _) => file.syncobj_create(arg),
        (0xc0, _) => file.syncobj_destroy(arg),
        (0xc3, _) => file.syncobj_wait(be, arg, user, false),
        (0xca, _) => file.syncobj_wait(be, arg, user, true),
        (0xc4, _) => file.syncobj_set(arg, user, None),
        (0xc5, _) => file.syncobj_set(arg, user, Some(super::sync::Point::Signaled)),
        (0xcb, _) => file.syncobj_query(be, arg, user),
        (0xcc, _) => file.syncobj_transfer(arg),
        (0xcd, _) => file.syncobj_timeline_signal(arg, user),
        (_, Some(super::XE_GEM_CREATE)) => file.gem_create(be, arg),
        (_, Some(super::XE_GEM_MMAP_OFFSET)) => file.gem_mmap_offset(arg),
        (_, Some(super::XE_VM_CREATE)) => file.vm_create(be, arg),
        (_, Some(super::XE_VM_DESTROY)) => file.vm_destroy(be, arg),
        (_, Some(super::XE_VM_BIND)) => file.vm_bind(be, arg, user),
        (_, Some(super::XE_EXEC_QUEUE_CREATE)) => file.queue_create(be, arg, user),
        (_, Some(super::XE_EXEC_QUEUE_DESTROY)) => file.queue_destroy(be, arg),
        (_, Some(super::XE_EXEC_QUEUE_GET_PROPERTY)) => file.queue_get_property(be, arg),
        (_, Some(super::XE_EXEC)) => file.exec(be, arg, user),
        (_, Some(super::XE_WAIT_USER_FENCE)) => file.wait_user_fence(be, arg, user),
        _ => Err(Errno::Inval),
    }
}
