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
//!
//! `/dev/ultar/pcm-mono`: the switch that sums both channels into each before they reach the
//! card (accessibility, "mono audio"); for every writer of the streams at once.
//!
//! * open: right `SOUND` only; any number of descriptors; no sound card: `ENODEV`;
//! * `write`: `1` switches it on, `0` off (the first byte counts); anything else `EINVAL`;
//! * `read`: `EINVAL`.

use super::{EAGAIN, EBUSY, EFAULT, EINVAL, ENODEV};
use crate::drivers::gen_hda::{self, PcmError};
use crate::proc::{self, FdKind};
use crate::syscall::with;
use alloc::sync::Arc;

/// Name below `/dev/`.
pub const NODE: &str = "ultar/pcm";
/// Name below `/dev/` of the mono switch.
pub const MONO_NODE: &str = "ultar/pcm-mono";
/// Largest single transfer (≈ 0.34 s).
const MAX_IO: usize = 64 * 1024;

/// The open node: the streams, or the mono switch.
pub struct PcmFd {
    switch: bool,
}

impl Drop for PcmFd {
    fn drop(&mut self) {
        if !self.switch {
            gen_hda::pcm_close();
        }
    }
}

/// Whether the node exists (a sound card with output is there).
pub fn present() -> bool {
    gen_hda::pcm_present()
}

/// Opens the node (the caller holds the right).
pub fn open() -> Result<FdKind, i64> {
    match gen_hda::pcm_open() {
        Ok(()) => Ok(FdKind::Pcm(Arc::new(PcmFd { switch: false }))),
        Err(PcmError::Gone) => Err(-ENODEV),
        Err(PcmError::Busy) => Err(-EBUSY),
    }
}

/// Opens the mono switch (the caller holds the right); not exclusive, the streams stay as they are.
pub fn open_mono() -> Result<FdKind, i64> {
    if !present() {
        return Err(-ENODEV);
    }
    Ok(FdKind::Pcm(Arc::new(PcmFd { switch: true })))
}

/// Sums the two channels of every whole frame (16 bit little endian, left then right) into both.
pub fn downmix(data: &mut [u8]) {
    for frame in data.chunks_exact_mut(4) {
        let left = i16::from_le_bytes([frame[0], frame[1]]) as i32;
        let right = i16::from_le_bytes([frame[2], frame[3]]) as i32;
        let both = (((left + right) / 2) as i16).to_le_bytes();
        frame.copy_from_slice(&[both[0], both[1], both[0], both[1]]);
    }
}

/// What a read or write on the node needs from the system call.
pub enum Io {
    /// Done: the result.
    Val(i64),
    /// Wait a little and repeat the call.
    Block,
}

/// `(nonblocking, the mono switch)` of the descriptor, if it is one of these nodes.
fn handle(fd: u64) -> Option<(bool, bool)> {
    with(|p| match p.fd(fd) {
        Some(d) => match &d.kind {
            FdKind::Pcm(f) => Some((d.flags & 0x800 != 0, f.switch)),
            _ => None,
        },
        None => None,
    })
    .flatten()
}

/// `read` on the node; `None` if `fd` is not it.
pub fn sys_read(fd: u64, buf: u64, n: u64) -> Option<Io> {
    let (nonblock, switch) = handle(fd)?;
    if switch {
        return Some(Io::Val(-EINVAL));
    }
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
    let (nonblock, switch) = handle(fd)?;
    if switch {
        return Some(Io::Val(switch_write(buf, n)));
    }
    let want = (n as usize).min(MAX_IO) & !3;
    if want == 0 {
        return Some(Io::Val(-EINVAL));
    }
    let Some(mut data) = with(|p| proc::read_user(p.space(), buf, want)).flatten() else {
        return Some(Io::Val(-EFAULT));
    };
    if gen_hda::mono() {
        downmix(&mut data);
    }
    let taken = gen_hda::pcm_write(&data);
    if taken == 0 {
        return Some(if nonblock { Io::Val(-EAGAIN) } else { Io::Block });
    }
    Some(Io::Val(taken as i64))
}

/// `write` on the mono switch: `1` on, `0` off.
fn switch_write(buf: u64, n: u64) -> i64 {
    if n == 0 {
        return -EINVAL;
    }
    let Some(first) = with(|p| proc::read_user(p.space(), buf, 1)).flatten() else {
        return -EFAULT;
    };
    match first.first() {
        Some(b'1') => gen_hda::set_mono(true),
        Some(b'0') => gen_hda::set_mono(false),
        _ => return -EINVAL,
    }
    n as i64
}

/// `poll` bits: readable, writable.
pub fn ready() -> (bool, bool) {
    (gen_hda::pcm_readable(), gen_hda::pcm_writable())
}
