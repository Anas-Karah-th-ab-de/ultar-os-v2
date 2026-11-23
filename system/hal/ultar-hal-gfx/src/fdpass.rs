//! Passing descriptors over a Unix stream socket (`SCM_RIGHTS`).
//!
//! The receiving side never keeps more than [`MAX_FDS_PER_READ`] descriptors
//! from one read: the control buffer has room for exactly that many, the
//! kernel closes what does not fit (`MSG_CTRUNC`), and every received
//! descriptor is close-on-exec. What the caller does not use is closed when
//! the returned `OwnedFd`s are dropped.

use rustix::net::{
    recvmsg, sendmsg, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags,
    SendAncillaryBuffer, SendAncillaryMessage, SendFlags,
};
use std::io::{IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;

/// Most descriptors taken from one read.
pub const MAX_FDS_PER_READ: usize = 4;

/// Result of one [`recv`].
#[derive(Debug, Default)]
pub struct Received {
    /// Bytes placed in the buffer (0 = the peer closed the connection).
    pub bytes: usize,
    /// Descriptors that came with these bytes.
    pub fds: Vec<OwnedFd>,
    /// The peer sent more descriptors than fit; the extra ones were closed.
    pub truncated: bool,
}

/// Sends `bytes` with at most one descriptor attached. Returns the number of
/// bytes the kernel accepted; the descriptor (if any) travels with the first
/// byte, so a caller that gets a short count must send the rest without it.
pub fn send(
    stream: &UnixStream,
    bytes: &[u8],
    fd: Option<BorrowedFd<'_>>,
) -> std::io::Result<usize> {
    let iov = [IoSlice::new(bytes)];
    let mut space = [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    let fds;
    if let Some(fd) = fd {
        fds = [fd];
        if !control.push(SendAncillaryMessage::ScmRights(&fds)) {
            return Err(std::io::Error::from_raw_os_error(105));
        }
    }
    Ok(sendmsg(
        stream,
        &iov,
        &mut control,
        SendFlags::NOSIGNAL | SendFlags::DONTWAIT,
    )?)
}

/// Reads into `buf` and collects the descriptors that came with the bytes.
pub fn recv(stream: &UnixStream, buf: &mut [u8]) -> std::io::Result<Received> {
    let mut iov = [IoSliceMut::new(buf)];
    let mut space = [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(MAX_FDS_PER_READ))];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    let message = recvmsg(
        stream,
        &mut iov,
        &mut control,
        RecvFlags::CMSG_CLOEXEC | RecvFlags::DONTWAIT,
    )?;
    let mut received = Received {
        bytes: message.bytes,
        fds: Vec::new(),
        truncated: message.flags.contains(ReturnFlags::CTRUNC),
    };
    for item in control.drain() {
        if let RecvAncillaryMessage::ScmRights(fds) = item {
            for fd in fds {
                if received.fds.len() < MAX_FDS_PER_READ {
                    received.fds.push(fd);
                } else {
                    received.truncated = true;
                }
            }
        }
    }
    Ok(received)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shm::{SealedMapping, SharedBuffer};
    use std::io::Write;
    use std::os::fd::AsFd;

    #[test]
    fn a_buffer_travels_with_its_bytes() {
        let (a, b) = UnixStream::pair().unwrap();
        let mut buffer = SharedBuffer::create(16).unwrap();
        buffer.bytes_mut()[0..4].copy_from_slice(&42u32.to_ne_bytes());
        assert_eq!(send(&a, b"hello", Some(buffer.fd())).unwrap(), 5);
        let mut buf = [0u8; 16];
        let got = recv(&b, &mut buf).unwrap();
        assert_eq!(&buf[..got.bytes], b"hello");
        assert_eq!(got.fds.len(), 1);
        assert!(!got.truncated);
        let mapping = SealedMapping::map(got.fds.into_iter().next().unwrap(), 16).unwrap();
        let mut px = [0u32; 1];
        mapping.copy_pixels(0, &mut px).unwrap();
        assert_eq!(px[0], 42);
    }

    #[test]
    fn plain_bytes_and_closed_peer() {
        let (mut a, b) = UnixStream::pair().unwrap();
        a.write_all(b"xy").unwrap();
        let mut buf = [0u8; 8];
        let got = recv(&b, &mut buf).unwrap();
        assert_eq!(got.bytes, 2);
        assert!(got.fds.is_empty());
        drop(a);
        assert_eq!(recv(&b, &mut buf).unwrap().bytes, 0);
        // Nothing to read on an open, idle socket: would block.
        let (_c, d) = UnixStream::pair().unwrap();
        let err = recv(&d, &mut buf).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
    }

    #[test]
    fn too_many_descriptors_are_cut_and_closed() {
        let (a, b) = UnixStream::pair().unwrap();
        let file = std::fs::File::open("/proc/self/stat").unwrap();
        let many: Vec<BorrowedFd<'_>> = (0..8).map(|_| file.as_fd()).collect();
        let iov = [IoSlice::new(b"z")];
        let mut space = [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(8))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        assert!(control.push(SendAncillaryMessage::ScmRights(&many)));
        sendmsg(&a, &iov, &mut control, SendFlags::NOSIGNAL).unwrap();
        let mut buf = [0u8; 4];
        let got = recv(&b, &mut buf).unwrap();
        assert_eq!(got.bytes, 1);
        assert!(got.fds.len() <= MAX_FDS_PER_READ);
        assert!(got.truncated);
    }
}
