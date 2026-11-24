//! The DHCP client's socket: UDP port 68 on all addresses, bound to one
//! interface, broadcast allowed – so it works before the interface has an
//! address (the client asks servers to answer by broadcast).

use crate::{sys, NetError};
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::os::fd::{AsFd, BorrowedFd};

/// DHCP client port.
pub const CLIENT_PORT: u16 = 68;
/// DHCP server port.
pub const SERVER_PORT: u16 = 67;

/// UDP 68 on one interface.
#[derive(Debug)]
pub struct DhcpSocket {
    socket: UdpSocket,
}

impl DhcpSocket {
    /// Opens the socket on interface `name` (needs the network right).
    pub fn open(name: &str) -> Result<DhcpSocket, NetError> {
        let s = rustix::net::socket_with(
            rustix::net::AddressFamily::INET,
            rustix::net::SocketType::DGRAM,
            rustix::net::SocketFlags::CLOEXEC,
            None,
        )?;
        rustix::net::sockopt::set_socket_reuseaddr(&s, true)?;
        rustix::net::sockopt::set_socket_broadcast(&s, true)?;
        sys::bind_to_device(s.as_fd(), name)?;
        rustix::net::bind(&s, &SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, CLIENT_PORT))?;
        let socket = UdpSocket::from(s);
        socket
            .set_nonblocking(true)
            .map_err(|e| NetError::Kernel(e.raw_os_error().unwrap_or(libc::EIO)))?;
        Ok(DhcpSocket { socket })
    }

    /// Sends to the broadcast address or a server.
    pub fn send(&self, to: Ipv4Addr, bytes: &[u8]) -> Result<(), NetError> {
        self.socket
            .send_to(bytes, SocketAddrV4::new(to, SERVER_PORT))
            .map(|_| ())
            .map_err(|e| NetError::Kernel(e.raw_os_error().unwrap_or(libc::EIO)))
    }

    /// Receives one waiting datagram from port 67 (`None`: none waiting,
    /// or one from another port).
    pub fn recv(&self, buf: &mut [u8]) -> Result<Option<usize>, NetError> {
        match self.socket.recv_from(buf) {
            Ok((n, from)) if from.port() == SERVER_PORT => Ok(Some(n)),
            Ok(_) => Ok(None),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(NetError::Kernel(e.raw_os_error().unwrap_or(libc::EIO))),
        }
    }
}

impl AsFd for DhcpSocket {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.socket.as_fd()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_a_real_interface() {
        assert!(DhcpSocket::open("no-such-if0").is_err());
        assert!(DhcpSocket::open("").is_err());
    }
}
