//! `/dev/ultar/kamera`: the pictures of the first USB camera for the call service
//! (drivers/gen_xhci/uvc.rs): one JPEG (MJPEG frame, maybe without Huffman tables) per read.
//!
//! * open: right `CAMERA` only; one descriptor at a time (`EBUSY`); no camera: `ENODEV`; the
//!   camera refuses the stream: `EIO`. Opening starts the stream, the last close stops it;
//! * `read`: the newest picture not read yet, whole (a buffer smaller than the picture: `ENOSPC`,
//!   the picture is dropped); none yet: waits, or `EAGAIN` with `O_NONBLOCK`; the camera left:
//!   `ENODEV`;
//! * `poll`: `POLLIN` when a new picture waits; `write` is not offered.

use super::{EAGAIN, EBUSY, EFAULT, EINVAL, EIO, ENODEV, ENOSPC};
use crate::drivers::gen_xhci::uvc::{self, CamError};
use crate::proc::{self, FdKind};
use crate::syscall::with;
use alloc::sync::Arc;

/// Name below `/dev/`.
pub const NODE: &str = "ultar/kamera";

/// The open node: the camera's serial number.
pub struct KameraFd(pub u64);

impl Drop for KameraFd {
    fn drop(&mut self) {
        uvc::close(self.0);
    }
}

/// Whether the node exists (a camera is there).
pub fn present() -> bool {
    uvc::present()
}

/// Opens the node (the caller holds the right).
pub fn open() -> Result<FdKind, i64> {
    match uvc::open() {
        Ok(id) => Ok(FdKind::Kamera(Arc::new(KameraFd(id)))),
        Err(CamError::Gone) => Err(-ENODEV),
        Err(CamError::Busy) => Err(-EBUSY),
        Err(CamError::Io) => Err(-EIO),
    }
}

/// What a read needs from the system call.
pub enum Io {
    /// Done: the result.
    Val(i64),
    /// Wait a little and repeat the call.
    Block,
}

fn handle(fd: u64) -> Option<(u64, bool)> {
    with(|p| match p.fd(fd) {
        Some(d) => match &d.kind {
            FdKind::Kamera(k) => Some((k.0, d.flags & 0x800 != 0)),
            _ => None,
        },
        None => None,
    })
    .flatten()
}

/// `read` on the node; `None` if `fd` is not it.
pub fn sys_read(fd: u64, buf: u64, n: u64) -> Option<Io> {
    let (id, nonblock) = handle(fd)?;
    if n == 0 {
        return Some(Io::Val(-EINVAL));
    }
    uvc::poll();
    let pic = match uvc::take(id) {
        Ok(Some(p)) => p,
        Ok(None) => return Some(if nonblock { Io::Val(-EAGAIN) } else { Io::Block }),
        Err(_) => return Some(Io::Val(-ENODEV)),
    };
    if pic.len() as u64 > n {
        return Some(Io::Val(-ENOSPC));
    }
    if !with(|p| proc::write_user(p.space(), buf, &pic)).unwrap_or(false) {
        return Some(Io::Val(-EFAULT));
    }
    Some(Io::Val(pic.len() as i64))
}

/// `poll` bit: a new picture is there.
pub fn ready(k: &KameraFd) -> bool {
    uvc::poll();
    uvc::ready(k.0)
}
