//! The requests every DRM driver answers (version, capabilities) and the entry point that
//! takes any request on the device file. Layouts: `include/uapi/drm/drm.h` (MIT-style licence).

use super::{Answer, Device, DeviceQuery, Errno, Request, XE_DEVICE_QUERY};
use alloc::vec::Vec;

/// Request: driver name and version.
pub const DRM_VERSION: u32 = 0x00;
/// Request: one capability.
pub const DRM_GET_CAP: u32 = 0x0c;

/// Capability: time stamps are monotonic.
pub const CAP_TIMESTAMP_MONOTONIC: u64 = 0x6;
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
        CAP_TIMESTAMP_MONOTONIC => 1,
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

/// Handles request `req` on the device file. `arg` is the request's structure, copied from the
/// program ([`Request::size`] bytes); the caller copies it back when the request reads
/// ([`Request::read`]) and this returns `Ok`. A number that is not DRM's is refused with
/// [`Errno::NoTty`], a DRM request the kernel does not have yet with [`Errno::Inval`].
pub fn ioctl(dev: &Device, req: u32, arg: &mut [u8], user: &mut impl User) -> Result<(), Errno> {
    let r = Request::decode(req).ok_or(Errno::NoTty)?;
    if arg.len() < r.size as usize {
        return Err(Errno::Inval);
    }
    match (r.nr, r.driver()) {
        (DRM_VERSION, _) => version(arg, user),
        (DRM_GET_CAP, _) => get_cap(arg),
        (_, Some(XE_DEVICE_QUERY)) => device_query(dev, arg, user),
        _ => Err(Errno::Inval),
    }
}
