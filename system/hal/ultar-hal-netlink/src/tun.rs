//! TUN devices: a layer-3 interface whose packets this process reads and
//! writes (the VPN tunnel).

use crate::{sys, NetError};
use rustix::fs::{Mode, OFlags};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};

const IFF_TUN: i16 = 0x0001;
const IFF_NO_PI: i16 = 0x1000;

/// Where the TUN control device is below a root directory.
pub fn control_path(root: &Path) -> PathBuf {
    root.join("dev/net/tun")
}

/// An open TUN interface.
#[derive(Debug)]
pub struct Tun {
    fd: OwnedFd,
    name: String,
}

impl Tun {
    /// Creates (or attaches to) the TUN interface `name` through the
    /// control device below `root`; [`NetError::Unavailable`] when the
    /// kernel has no TUN.
    pub fn open(root: &Path, name: &str) -> Result<Tun, NetError> {
        let fd = rustix::fs::open(
            control_path(root),
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|e| match NetError::from(e) {
            NetError::Kernel(_) if e == rustix::io::Errno::NXIO => NetError::Unavailable,
            other => other,
        })?;
        let name =
            sys::tun_set_iff(fd.as_fd(), name, IFF_TUN | IFF_NO_PI).map_err(NetError::from)?;
        Ok(Tun { fd, name })
    }

    /// The interface name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The descriptor (to wait on it).
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }

    /// Reads one packet (`None`: nothing waiting).
    pub fn read(&self, buf: &mut [u8]) -> Result<Option<usize>, NetError> {
        match rustix::io::read(&self.fd, buf) {
            Ok(n) => Ok(Some(n)),
            Err(rustix::io::Errno::AGAIN) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Writes one packet.
    pub fn write(&self, packet: &[u8]) -> Result<(), NetError> {
        rustix::io::write(&self.fd, packet)
            .map(|_| ())
            .map_err(NetError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_without_tun_is_unavailable() {
        let none = std::env::temp_dir().join(format!("tun-none-{}", std::process::id()));
        assert_eq!(Tun::open(&none, "t0").err(), Some(NetError::Unavailable));
        assert_eq!(control_path(Path::new("/")), PathBuf::from("/dev/net/tun"));
    }
}
