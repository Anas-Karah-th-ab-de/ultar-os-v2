//! The ALSA control ioctls (`include/uapi/sound/asound.h`) on byte buffers of the
//! structures' exact sizes (64-bit layout, x86_64 and aarch64 alike). None of the
//! structures used here holds a pointer the kernel would follow: `ELEM_LIST` is only
//! issued with `space = 0` and a null `pids`, so the kernel only writes the counts.
#![allow(unsafe_code)]

use rustix::ioctl::{ioctl, opcode, Getter, Updater};
use std::os::fd::AsFd;

/// `struct snd_ctl_card_info`.
pub const CARD_INFO_SIZE: usize = 376;
/// `struct snd_ctl_elem_list`.
pub const LIST_SIZE: usize = 80;
/// `struct snd_ctl_elem_info`.
pub const INFO_SIZE: usize = 272;
/// `struct snd_ctl_elem_value`.
pub const VALUE_SIZE: usize = 1224;

const CARD_INFO: rustix::ioctl::Opcode = opcode::read::<[u8; CARD_INFO_SIZE]>(b'U', 0x01);
const ELEM_LIST: rustix::ioctl::Opcode = opcode::read_write::<[u8; LIST_SIZE]>(b'U', 0x10);
const ELEM_INFO: rustix::ioctl::Opcode = opcode::read_write::<[u8; INFO_SIZE]>(b'U', 0x11);
const ELEM_READ: rustix::ioctl::Opcode = opcode::read_write::<[u8; VALUE_SIZE]>(b'U', 0x12);
const ELEM_WRITE: rustix::ioctl::Opcode = opcode::read_write::<[u8; VALUE_SIZE]>(b'U', 0x13);

/// `SNDRV_CTL_IOCTL_CARD_INFO`.
pub fn card_info(fd: impl AsFd) -> rustix::io::Result<[u8; CARD_INFO_SIZE]> {
    // SAFETY: the opcode encodes the size of the output; the structure has no pointers.
    unsafe { ioctl(fd, Getter::<CARD_INFO, [u8; CARD_INFO_SIZE]>::new()) }
}

/// `SNDRV_CTL_IOCTL_ELEM_LIST` with `space = 0`: the number of elements.
pub fn elem_count(fd: impl AsFd) -> rustix::io::Result<u32> {
    // offset = 0, space = 0, pids = NULL: the kernel fills `count` only.
    let mut buf = [0u8; LIST_SIZE];
    // SAFETY: size from the opcode; `pids` is null and `space` 0, so the kernel
    // writes no element ids anywhere but into this buffer's counters.
    unsafe { ioctl(&fd, Updater::<ELEM_LIST, [u8; LIST_SIZE]>::new(&mut buf)) }?;
    Ok(u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]))
}

/// `SNDRV_CTL_IOCTL_ELEM_INFO` on a prepared `snd_ctl_elem_info`.
pub fn elem_info(fd: impl AsFd, buf: &mut [u8; INFO_SIZE]) -> rustix::io::Result<()> {
    // SAFETY: size from the opcode; the structure has no pointers.
    unsafe { ioctl(&fd, Updater::<ELEM_INFO, [u8; INFO_SIZE]>::new(buf)) }
}

/// `SNDRV_CTL_IOCTL_ELEM_READ` on a prepared `snd_ctl_elem_value`.
pub fn elem_read(fd: impl AsFd, buf: &mut [u8; VALUE_SIZE]) -> rustix::io::Result<()> {
    // SAFETY: size from the opcode; the structure has no pointers.
    unsafe { ioctl(&fd, Updater::<ELEM_READ, [u8; VALUE_SIZE]>::new(buf)) }
}

/// `SNDRV_CTL_IOCTL_ELEM_WRITE` of a filled `snd_ctl_elem_value`.
pub fn elem_write(fd: impl AsFd, buf: &mut [u8; VALUE_SIZE]) -> rustix::io::Result<()> {
    // SAFETY: size from the opcode; the structure has no pointers.
    unsafe { ioctl(&fd, Updater::<ELEM_WRITE, [u8; VALUE_SIZE]>::new(buf)) }
}
