//! The ALSA PCM ioctls (`include/uapi/sound/asound.h`) on byte buffers of the structures'
//! exact sizes (64-bit layout, x86_64 and aarch64 alike). The samples themselves go through
//! `read(2)` and `write(2)` (interleaved access), so no structure used here holds a pointer
//! the kernel would follow.
#![allow(unsafe_code)]

use rustix::ioctl::{ioctl, opcode, Getter, NoArg, Opcode, Updater};
use std::os::fd::AsFd;

/// `struct snd_pcm_hw_params`.
pub const HW_PARAMS_SIZE: usize = 608;

/// `SNDRV_PCM_IOCTL_HW_PARAMS`.
pub const HW_PARAMS: Opcode = opcode::read_write::<[u8; HW_PARAMS_SIZE]>(b'A', 0x11);
/// `SNDRV_PCM_IOCTL_DELAY`: frames queued in a playback stream.
pub const DELAY: Opcode = opcode::read::<i64>(b'A', 0x21);
/// `SNDRV_PCM_IOCTL_PREPARE`.
pub const PREPARE: Opcode = opcode::none(b'A', 0x40);
/// `SNDRV_PCM_IOCTL_DROP`.
pub const DROP: Opcode = opcode::none(b'A', 0x43);

/// Asks for the parameters in `buf` and gets what the driver chose back.
pub fn hw_params(fd: impl AsFd, buf: &mut [u8; HW_PARAMS_SIZE]) -> rustix::io::Result<()> {
    // SAFETY: size from the opcode; the structure has no pointers.
    unsafe { ioctl(&fd, Updater::<HW_PARAMS, [u8; HW_PARAMS_SIZE]>::new(buf)) }
}

/// Frames the stream holds (negative: an error state the driver reports as a count).
pub fn delay(fd: impl AsFd) -> rustix::io::Result<i64> {
    // SAFETY: the opcode encodes the size of the output, a plain integer.
    unsafe { ioctl(&fd, Getter::<DELAY, i64>::new()) }
}

/// Makes the stream ready (also the way out of an under- or overrun).
pub fn prepare(fd: impl AsFd) -> rustix::io::Result<()> {
    // SAFETY: no argument.
    unsafe { ioctl(&fd, NoArg::<PREPARE>::new()) }
}

/// Stops the stream at once and throws away what it holds.
pub fn drop_stream(fd: impl AsFd) -> rustix::io::Result<()> {
    // SAFETY: no argument.
    unsafe { ioctl(&fd, NoArg::<DROP>::new()) }
}
