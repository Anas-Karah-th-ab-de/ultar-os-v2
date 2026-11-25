//! `/dev/ultar/pcm`: the sound card's running streams for the call service (drivers/gen_hda):
//! 48 kHz, 16 bit, stereo interleaved, little endian.
//!
//! * open: right `SOUND` only; one descriptor at a time (`EBUSY`); no sound card: `ENODEV`;
//! * `write`: queues as much as fits (up to 200 ms ahead, whole frames), returns the bytes taken;
//!   nothing fits: waits, or `EAGAIN` with `O_NONBLOCK`;
//! * `read`: the recorded sound that is there (whole frames); nothing: waits, or `EAGAIN`;
//!   a codec without microphone or line in records nothing (`read` waits for ever / `EAGAIN`);
//! * `poll`: `POLLIN` when recorded sound waits, `POLLOUT` when the output has room;
//! * the last close stops the streams.

use super::{EAGAIN, EBUSY, EFAULT, EINVAL, ENODEV};
use crate::drivers::gen_hda::{self, PcmError};
use crate::proc::{self, FdKind};
use crate::syscall::with;
use alloc::sync::Arc;

/// Name below `/dev/`.
pub const NODE: &str = "ultar/pcm";
/// Largest single transfer (≈ 0.34 s).
const MAX_IO: usize = 64 * 1024;

/// The open node.
pub struct PcmFd;

impl Drop for PcmFd {
    fn drop(&mut self) {
        gen_hda::pcm_close();
    }
}

/// Whether the node exists (a sound card with output is there).
pub fn present() -> bool {
    gen_hda::pcm_present()
}

/// Opens the node (the caller holds the right).
pub fn open() -> Result<FdKind, i64> {
    match gen_hda::pcm_open() {
        Ok(()) => Ok(FdKind::Pcm(Arc::new(PcmFd))),
        Err(PcmError::Gone) => Err(-ENODEV),
        Err(PcmError::Busy) => Err(-EBUSY),
    }
}

/// What a read or write on the node needs from the system call.
pub enum Io {
    /// Done: the result.
    Val(i64),
    /// Wait a little and repeat the call.
    Block,
}

fn handle(fd: u64) -> Option<bool> {
    with(|p| match p.fd(fd) {
        Some(d) => match &d.kind {
            FdKind::Pcm(_) => Some(d.flags & 0x800 != 0),
            _ => None,
        },
        None => None,
    })
    .flatten()
}

/// `read` on the node; `None` if `fd` is not it.
pub fn sys_read(fd: u64, buf: u64, n: u64) -> Option<Io> {
    let nonblock = handle(fd)?;
    let want = (n as usize).min(MAX_IO) & !3;
    if want == 0 {
        return Some(Io::Val(-EINVAL));
    }
    let mut data = alloc::vec![0u8; want];
    let got = gen_hda::pcm_read(&mut data);
    if got == 0 {
        return Some(if nonblock { Io::Val(-EAGAIN) } else { Io::Block });
    }
    if !with(|p| proc::write_user(p.space(), buf, &data[..got])).unwrap_or(false) {
        return Some(Io::Val(-EFAULT));
    }
    Some(Io::Val(got as i64))
}

/// `write` on the node; `None` if `fd` is not it.
pub fn sys_write(fd: u64, buf: u64, n: u64) -> Option<Io> {
    let nonblock = handle(fd)?;
    let want = (n as usize).min(MAX_IO) & !3;
    if want == 0 {
        return Some(Io::Val(-EINVAL));
    }
    let Some(data) = with(|p| proc::read_user(p.space(), buf, want)).flatten() else {
        return Some(Io::Val(-EFAULT));
    };
    let taken = gen_hda::pcm_write(&data);
    if taken == 0 {
        return Some(if nonblock { Io::Val(-EAGAIN) } else { Io::Block });
    }
    Some(Io::Val(taken as i64))
}

/// `poll` bits: readable, writable.
pub fn ready() -> (bool, bool) {
    (gen_hda::pcm_readable(), gen_hda::pcm_writable())
}
