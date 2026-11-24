//! The only `unsafe` code of the crate: two socket options rustix does not
//! offer (netlink group membership, binding to a device) and the TUN ioctl.
//! Each call passes a pointer to a local, fixed-size, initialised buffer and
//! its exact length; the kernel copies from or into it and keeps nothing.
#![allow(unsafe_code)]

use rustix::io::Errno;
use std::os::fd::{AsRawFd, BorrowedFd};

/// `SOL_NETLINK`.
const SOL_NETLINK: libc::c_int = 270;
/// `NETLINK_ADD_MEMBERSHIP`.
const NETLINK_ADD_MEMBERSHIP: libc::c_int = 1;
/// `TUNSETIFF` (`_IOW('T', 202, int)`).
const TUNSETIFF: libc::c_ulong = 0x4004_54ca;
/// Length of an interface name with its NUL.
pub const IFNAMSIZ: usize = 16;

fn last() -> Errno {
    Errno::from_raw_os_error(
        std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO),
    )
}

/// Joins netlink multicast group `group`.
pub fn add_membership(fd: BorrowedFd<'_>, group: u32) -> Result<(), Errno> {
    let value: u32 = group;
    // SAFETY: `value` is a live local u32; the length passed is exactly its
    // size; the kernel only reads it during the call.
    let r = unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            SOL_NETLINK,
            NETLINK_ADD_MEMBERSHIP,
            (&value as *const u32).cast(),
            std::mem::size_of::<u32>() as libc::socklen_t,
        )
    };
    if r == 0 {
        Ok(())
    } else {
        Err(last())
    }
}

/// Binds a socket to one interface (`SO_BINDTODEVICE`).
pub fn bind_to_device(fd: BorrowedFd<'_>, name: &str) -> Result<(), Errno> {
    let mut buf = [0u8; IFNAMSIZ];
    if name.is_empty() || name.len() >= IFNAMSIZ || name.as_bytes().contains(&0) {
        return Err(Errno::INVAL);
    }
    buf[..name.len()].copy_from_slice(name.as_bytes());
    // SAFETY: `buf` is a live local array of IFNAMSIZ bytes, NUL-terminated
    // (checked above); the length passed is the name with its NUL.
    let r = unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_BINDTODEVICE,
            buf.as_ptr().cast(),
            (name.len() + 1) as libc::socklen_t,
        )
    };
    if r == 0 {
        Ok(())
    } else {
        Err(last())
    }
}

/// `struct ifreq` as `TUNSETIFF` reads and writes it: the name and the
/// flags, padded to the kernel's 40 bytes.
#[repr(C)]
struct IfReq {
    name: [u8; IFNAMSIZ],
    flags: libc::c_short,
    pad: [u8; 22],
}

/// Attaches an opened `/dev/net/tun` to interface `name` (empty: the kernel
/// picks one) with `flags`; returns the interface name.
pub fn tun_set_iff(fd: BorrowedFd<'_>, name: &str, flags: i16) -> Result<String, Errno> {
    if name.len() >= IFNAMSIZ || name.as_bytes().contains(&0) {
        return Err(Errno::INVAL);
    }
    let mut req = IfReq {
        name: [0; IFNAMSIZ],
        flags,
        pad: [0; 22],
    };
    req.name[..name.len()].copy_from_slice(name.as_bytes());
    // SAFETY: `req` is a live, fully initialised local of the size the
    // kernel expects for TUNSETIFF (40 bytes); the kernel reads it and
    // writes the chosen name back into it during the call only.
    let r = unsafe { libc::ioctl(fd.as_raw_fd(), TUNSETIFF as _, &mut req as *mut IfReq) };
    if r != 0 {
        return Err(last());
    }
    let end = req.name.iter().position(|b| *b == 0).unwrap_or(IFNAMSIZ);
    String::from_utf8(req.name[..end].to_vec()).map_err(|_| Errno::INVAL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsFd;

    #[test]
    fn sizes_and_refusals() {
        assert_eq!(std::mem::size_of::<IfReq>(), 40);
        let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        assert_eq!(bind_to_device(s.as_fd(), ""), Err(Errno::INVAL));
        assert_eq!(
            bind_to_device(s.as_fd(), &"x".repeat(16)),
            Err(Errno::INVAL)
        );
        assert_eq!(
            tun_set_iff(s.as_fd(), &"x".repeat(16), 1),
            Err(Errno::INVAL)
        );
        // Not a TUN device: the kernel refuses.
        assert!(tun_set_iff(s.as_fd(), "t0", 1).is_err());
        // Not a netlink socket.
        assert!(add_membership(s.as_fd(), 1).is_err());
    }
}
