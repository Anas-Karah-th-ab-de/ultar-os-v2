//! Pixel memory shared between an app process and the compositor.
//!
//! The app creates a [`SharedBuffer`] (an anonymous memory file, sealed so
//! that it can never shrink or grow), draws into it and passes the
//! descriptor to the compositor ([`crate::fdpass`]). The compositor maps it
//! read-only as a [`SealedMapping`] and **copies** the damaged rows out when
//! the app commits; it never hands out a reference into memory another
//! process can write. The seal check is what makes the mapping safe for the
//! compositor: a buffer that could be truncated would raise `SIGBUS` in the
//! compositor on the next read.

use crate::GfxError;
use rustix::fs::{fcntl_add_seals, fcntl_get_seals, fstat, ftruncate, memfd_create};
use rustix::fs::{MemfdFlags, SealFlags};
use rustix::mm::{mmap, munmap, MapFlags, ProtFlags};
use std::ffi::c_void;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::ptr::NonNull;

/// Largest shared buffer accepted or created: 256 MiB (an 8K frame).
pub const MAX_SHARED_BYTES: u64 = 256 << 20;

/// Seals every buffer carries: no shrinking, no growing, no further seal changes.
const REQUIRED_SEALS: SealFlags = SealFlags::SHRINK
    .union(SealFlags::GROW)
    .union(SealFlags::SEAL);

/// A writable shared pixel buffer owned by the app that draws into it.
#[derive(Debug)]
pub struct SharedBuffer {
    fd: OwnedFd,
    ptr: NonNull<c_void>,
    len: usize,
}

// SAFETY: the mapping is owned exclusively by this value (no aliases are
// handed out beyond borrows of `self`), so moving it to another thread is as
// safe as moving a `Vec<u8>`.
unsafe impl Send for SharedBuffer {}

impl SharedBuffer {
    /// Creates a sealed shared buffer of `len` bytes, zero-filled.
    pub fn create(len: usize) -> Result<SharedBuffer, GfxError> {
        if len == 0 || len as u64 > MAX_SHARED_BYTES {
            return Err(GfxError::InvalidArgument);
        }
        let fd = memfd_create(
            "ultar-surface",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )?;
        ftruncate(&fd, len as u64)?;
        fcntl_add_seals(&fd, REQUIRED_SEALS)?;
        // SAFETY: a fresh mapping (null hint, kernel chooses the address) of
        // `len` bytes of a file that is exactly `len` bytes long and sealed
        // against shrinking; nothing else in this process refers to it.
        let raw = unsafe {
            mmap(
                std::ptr::null_mut(),
                len,
                ProtFlags::READ | ProtFlags::WRITE,
                MapFlags::SHARED,
                &fd,
                0,
            )
        }?;
        let ptr = NonNull::new(raw).ok_or(GfxError::Sys(12))?;
        Ok(SharedBuffer { fd, ptr, len })
    }

    /// The descriptor to pass to the compositor.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }

    /// Size in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Always false: a buffer has at least one byte.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The pixels for drawing.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: `ptr` maps `len` readable and writable bytes for the
        // lifetime of `self`; the only other process that maps the file maps
        // it read-only, so no write can happen behind this exclusive borrow.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr().cast::<u8>(), self.len) }
    }

    /// The pixels for reading.
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: as in `bytes_mut`; shared borrow of memory only this
        // process writes.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr().cast::<u8>(), self.len) }
    }
}

impl Drop for SharedBuffer {
    fn drop(&mut self) {
        // SAFETY: `ptr`/`len` are exactly the mapping created in `create`,
        // and no borrow of it can outlive `self`.
        let _ = unsafe { munmap(self.ptr.as_ptr(), self.len) };
    }
}

/// A read-only view of a buffer another process shares with the compositor.
#[derive(Debug)]
pub struct SealedMapping {
    ptr: NonNull<c_void>,
    len: usize,
}

// SAFETY: the mapping is private to this value and only read through
// `copy_out`, which copies bytes; it can move between threads like a `Vec`.
unsafe impl Send for SealedMapping {}

impl SealedMapping {
    /// Maps a descriptor received from a client. Refuses descriptors that
    /// are not sealed against shrinking (not a memory file, or the client
    /// left the seal off), that are smaller than `min_len` or larger than
    /// [`MAX_SHARED_BYTES`]. The descriptor is closed when this returns; the
    /// mapping keeps the memory alive.
    pub fn map(fd: OwnedFd, min_len: u64) -> Result<SealedMapping, GfxError> {
        let seals = fcntl_get_seals(&fd).map_err(|_| GfxError::NotSealed)?;
        if !seals.contains(SealFlags::SHRINK) {
            return Err(GfxError::NotSealed);
        }
        let size = u64::try_from(fstat(&fd)?.st_size).map_err(|_| GfxError::InvalidArgument)?;
        if size > MAX_SHARED_BYTES {
            return Err(GfxError::TooLarge);
        }
        if size < min_len || size == 0 {
            return Err(GfxError::TooSmall {
                needed: min_len,
                actual: size,
            });
        }
        let len = usize::try_from(size).map_err(|_| GfxError::TooLarge)?;
        // SAFETY: read-only shared mapping of the whole file; the file can no
        // longer shrink (checked seal), so every byte of the mapping stays
        // backed and reads cannot fault with SIGBUS.
        let raw = unsafe {
            mmap(
                std::ptr::null_mut(),
                len,
                ProtFlags::READ,
                MapFlags::SHARED,
                &fd,
                0,
            )
        }?;
        let ptr = NonNull::new(raw).ok_or(GfxError::Sys(12))?;
        Ok(SealedMapping { ptr, len })
    }

    /// Size of the mapping in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Always false: an empty file is refused by [`SealedMapping::map`].
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Copies `dst.len()` pixels (4 bytes each, native byte order) starting
    /// at byte `offset` into `dst`.
    pub fn copy_pixels(&self, offset: usize, dst: &mut [u32]) -> Result<(), GfxError> {
        let bytes = dst.len().checked_mul(4).ok_or(GfxError::InvalidArgument)?;
        let end = offset.checked_add(bytes).ok_or(GfxError::InvalidArgument)?;
        if end > self.len {
            return Err(GfxError::InvalidArgument);
        }
        // The other process may write these bytes at the same time: only raw
        // bytes are copied (any bit pattern is a valid `u32`) and no
        // reference into the shared memory is ever created.
        // SAFETY: `offset..end` lies inside the mapping (checked above);
        // `dst` is our own memory and does not overlap the mapping.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.ptr.as_ptr().cast::<u8>().add(offset),
                dst.as_mut_ptr().cast::<u8>(),
                bytes,
            );
        }
        Ok(())
    }
}

impl Drop for SealedMapping {
    fn drop(&mut self) {
        // SAFETY: exactly the mapping created in `map`; `copy_pixels` never
        // leaks a pointer into it.
        let _ = unsafe { munmap(self.ptr.as_ptr(), self.len) };
    }
}

/// Whether this kernel supports sealed shared buffers end to end (create,
/// seal, map read-only from a second descriptor). False on the Ultar kernel
/// as of 23.11.2026 (`memfd_create` is missing).
pub fn probe() -> bool {
    let Ok(mut buffer) = SharedBuffer::create(4096) else {
        return false;
    };
    buffer.bytes_mut()[..4].copy_from_slice(&0x1234_5678u32.to_ne_bytes());
    let Ok(copy) = buffer.fd().try_clone_to_owned() else {
        return false;
    };
    let Ok(mapping) = SealedMapping::map(copy, 4096) else {
        return false;
    };
    let mut first = [0u32; 1];
    mapping.copy_pixels(0, &mut first).is_ok() && first[0] == 0x1234_5678
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn create_write_map_and_copy() {
        assert!(probe());
        let mut buffer = SharedBuffer::create(64).unwrap();
        assert_eq!(buffer.len(), 64);
        assert!(!buffer.is_empty());
        buffer.bytes_mut()[8..12].copy_from_slice(&7u32.to_ne_bytes());
        assert_eq!(buffer.bytes()[8], 7);
        let mapping = SealedMapping::map(buffer.fd().try_clone_to_owned().unwrap(), 64).unwrap();
        assert_eq!(mapping.len(), 64);
        assert!(!mapping.is_empty());
        let mut out = [0u32; 2];
        mapping.copy_pixels(8, &mut out).unwrap();
        assert_eq!(out, [7, 0]);
        // Writes after mapping are visible (shared, not copied).
        buffer.bytes_mut()[12] = 9;
        mapping.copy_pixels(8, &mut out).unwrap();
        assert_eq!(out[1], 9);
    }

    #[test]
    fn bounds_and_size_limits() {
        assert_eq!(
            SharedBuffer::create(0).err(),
            Some(GfxError::InvalidArgument)
        );
        assert_eq!(
            SharedBuffer::create(MAX_SHARED_BYTES as usize + 1).err(),
            Some(GfxError::InvalidArgument)
        );
        let buffer = SharedBuffer::create(16).unwrap();
        let mapping = SealedMapping::map(buffer.fd().try_clone_to_owned().unwrap(), 16).unwrap();
        let mut out = [0u32; 4];
        assert!(mapping.copy_pixels(0, &mut out).is_ok());
        assert_eq!(
            mapping.copy_pixels(4, &mut out),
            Err(GfxError::InvalidArgument)
        );
        assert_eq!(
            mapping.copy_pixels(usize::MAX - 2, &mut out),
            Err(GfxError::InvalidArgument)
        );
        let small = SealedMapping::map(buffer.fd().try_clone_to_owned().unwrap(), 17);
        assert_eq!(
            small.err(),
            Some(GfxError::TooSmall {
                needed: 17,
                actual: 16
            })
        );
    }

    #[test]
    fn unsealed_or_foreign_descriptors_are_refused() {
        // A memory file without seals.
        let fd = memfd_create("t", MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING).unwrap();
        ftruncate(&fd, 64).unwrap();
        assert_eq!(SealedMapping::map(fd, 16).err(), Some(GfxError::NotSealed));
        // A regular file.
        let dir = std::env::temp_dir().join(format!("ultar-gfx-shm-{}", std::process::id()));
        let mut file = std::fs::File::create(&dir).unwrap();
        file.write_all(&[0u8; 64]).unwrap();
        let owned: OwnedFd = file.into();
        assert_eq!(
            SealedMapping::map(owned, 16).err(),
            Some(GfxError::NotSealed)
        );
        std::fs::remove_file(&dir).unwrap();
        // A pipe.
        let (reader, _writer) = std::io::pipe().unwrap();
        assert_eq!(
            SealedMapping::map(OwnedFd::from(reader), 1).err(),
            Some(GfxError::NotSealed)
        );
    }
}
