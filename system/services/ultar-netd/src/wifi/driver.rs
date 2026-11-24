//! What the Wi-Fi core needs from the kernel, as traits: the radio
//! ([`WifiDriver`], nl80211 on Linux) and the IP side ([`IpLayer`]: the DHCP
//! socket, addresses and routes through rtnetlink). Tests put an access point
//! and a DHCP server behind them.

use std::net::{IpAddr, Ipv4Addr};
use std::os::fd::BorrowedFd;
use ultar_hal_netlink::nl80211::{Bss, ConnectParams, Nl80211, WifiEvent, WifiInterface};
use ultar_hal_netlink::rtnl::{Origin, Route, Rtnl};
use ultar_hal_netlink::udp::DhcpSocket;
use ultar_hal_netlink::NetError;
use ultar_netproto::dhcp::Lease;

/// Metric of the Wi-Fi default route (wired links win).
pub const WIFI_METRIC: u32 = 600;

/// The radio.
pub trait WifiDriver: Send {
    /// Station interfaces.
    fn interfaces(&mut self) -> Result<Vec<WifiInterface>, NetError>;
    /// Starts a scan.
    fn trigger_scan(&mut self, ifindex: u32) -> Result<(), NetError>;
    /// Results of the last scan.
    fn scan_results(&mut self, ifindex: u32) -> Result<Vec<Bss>, NetError>;
    /// Associates.
    fn connect(&mut self, ifindex: u32, p: &ConnectParams) -> Result<(), NetError>;
    /// Leaves.
    fn disconnect(&mut self, ifindex: u32) -> Result<(), NetError>;
    /// Sends an EAPOL frame.
    fn send_eapol(
        &mut self,
        ifindex: u32,
        to: [u8; 6],
        frame: &[u8],
        encrypt: bool,
    ) -> Result<(), NetError>;
    /// Installs a key (pairwise with `peer`, else group).
    fn install_key(
        &mut self,
        ifindex: u32,
        peer: Option<[u8; 6]>,
        index: u8,
        key: &[u8],
        rsc: &[u8; 8],
    ) -> Result<(), NetError>;
    /// Opens the port.
    fn authorize(&mut self, ifindex: u32, peer: [u8; 6]) -> Result<(), NetError>;
    /// Signal of the access point (dBm).
    fn signal(&mut self, ifindex: u32, peer: [u8; 6]) -> Result<Option<i8>, NetError>;
    /// Next event (no wait).
    fn next_event(&mut self) -> Result<Option<WifiEvent>, NetError>;
    /// Descriptor to wait on.
    fn fd(&self) -> Option<BorrowedFd<'_>>;
}

impl WifiDriver for Nl80211 {
    fn interfaces(&mut self) -> Result<Vec<WifiInterface>, NetError> {
        Nl80211::interfaces(self)
    }
    fn trigger_scan(&mut self, ifindex: u32) -> Result<(), NetError> {
        Nl80211::trigger_scan(self, ifindex)
    }
    fn scan_results(&mut self, ifindex: u32) -> Result<Vec<Bss>, NetError> {
        Nl80211::scan_results(self, ifindex)
    }
    fn connect(&mut self, ifindex: u32, p: &ConnectParams) -> Result<(), NetError> {
        Nl80211::connect(self, ifindex, p)
    }
    fn disconnect(&mut self, ifindex: u32) -> Result<(), NetError> {
        Nl80211::disconnect(self, ifindex)
    }
    fn send_eapol(
        &mut self,
        ifindex: u32,
        to: [u8; 6],
        frame: &[u8],
        encrypt: bool,
    ) -> Result<(), NetError> {
        Nl80211::send_eapol(self, ifindex, to, frame, encrypt)
    }
    fn install_key(
        &mut self,
        ifindex: u32,
        peer: Option<[u8; 6]>,
        index: u8,
        key: &[u8],
        rsc: &[u8; 8],
    ) -> Result<(), NetError> {
        Nl80211::install_key(self, ifindex, peer, index, key, rsc)
    }
    fn authorize(&mut self, ifindex: u32, peer: [u8; 6]) -> Result<(), NetError> {
        Nl80211::authorize(self, ifindex, peer)
    }
    fn signal(&mut self, ifindex: u32, peer: [u8; 6]) -> Result<Option<i8>, NetError> {
        Nl80211::signal(self, ifindex, peer)
    }
    fn next_event(&mut self) -> Result<Option<WifiEvent>, NetError> {
        Nl80211::next_event(self, None)
    }
    fn fd(&self) -> Option<BorrowedFd<'_>> {
        Some(Nl80211::fd(self))
    }
}

/// The IP side of one interface.
pub trait IpLayer: Send {
    /// Opens the DHCP socket on `ifname`.
    fn open(&mut self, ifname: &str) -> Result<(), NetError>;
    /// Closes it.
    fn close(&mut self);
    /// Sends a DHCP message.
    fn send(&mut self, to: Ipv4Addr, bytes: &[u8]) -> Result<(), NetError>;
    /// A waiting DHCP answer (no wait).
    fn recv(&mut self, buf: &mut [u8]) -> Result<Option<usize>, NetError>;
    /// Puts the lease on `ifname`: address, default route.
    fn configure(&mut self, ifname: &str, lease: &Lease) -> Result<(), NetError>;
    /// Takes every IPv4 address of `ifname` away.
    fn deconfigure(&mut self, ifname: &str) -> Result<(), NetError>;
    /// Descriptor to wait on.
    fn fd(&self) -> Option<BorrowedFd<'_>>;
}

/// [`IpLayer`] of Linux.
#[derive(Debug, Default)]
pub struct LinuxIp {
    socket: Option<DhcpSocket>,
}

impl IpLayer for LinuxIp {
    fn open(&mut self, ifname: &str) -> Result<(), NetError> {
        self.socket = Some(DhcpSocket::open(ifname)?);
        Ok(())
    }
    fn close(&mut self) {
        self.socket = None;
    }
    fn send(&mut self, to: Ipv4Addr, bytes: &[u8]) -> Result<(), NetError> {
        self.socket
            .as_ref()
            .ok_or(NetError::NoDevice)?
            .send(to, bytes)
    }
    fn recv(&mut self, buf: &mut [u8]) -> Result<Option<usize>, NetError> {
        match &self.socket {
            Some(s) => s.recv(buf),
            None => Ok(None),
        }
    }
    fn configure(&mut self, ifname: &str, lease: &Lease) -> Result<(), NetError> {
        let mut rtnl = Rtnl::open()?;
        let link = rtnl.link(ifname)?;
        rtnl.flush_v4(link.index)?;
        rtnl.add_address(link.index, IpAddr::V4(lease.address), lease.prefix)?;
        if let Some(router) = lease.router {
            rtnl.add_route(&Route {
                dst: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                prefix: 0,
                gateway: Some(IpAddr::V4(router)),
                oif: link.index,
                metric: WIFI_METRIC,
                origin: Origin::Dhcp,
            })?;
        }
        Ok(())
    }
    fn deconfigure(&mut self, ifname: &str) -> Result<(), NetError> {
        let mut rtnl = Rtnl::open()?;
        let link = rtnl.link(ifname)?;
        rtnl.flush_v4(link.index)
    }
    fn fd(&self) -> Option<BorrowedFd<'_>> {
        use std::os::fd::AsFd;
        self.socket.as_ref().map(|s| s.as_fd())
    }
}
