//! One running WireGuard tunnel: a TUN interface with the tunnel's
//! addresses and routes, a UDP socket to the server, and boringtun's
//! protocol state (`Tunn`: handshake, keys, timers, encryption – reviewed
//! code, nothing of it rewritten here) on a thread of its own.
//!
//! Incoming packets are only written to the interface when their inner
//! source address lies in the peer's `AllowedIPs` (cryptokey routing).
//! For a full tunnel the default route is split in two halves through the
//! tunnel and the server keeps a host route through the old gateway, so the
//! tunnel's own packets never loop into it.

use crate::conf::{Net, TunnelConf};
use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use rustix::event::{poll, PollFd, PollFlags, Timespec};
use std::io::Read;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_hal_netlink::rtnl::{Origin, Route, Rtnl};
use ultar_hal_netlink::tun::Tun;
use ultar_hal_netlink::NetError;

/// Name of the tunnel interface.
pub const IFNAME: &str = "ultar-vpn0";
/// MTU when the file gives none.
pub const DEFAULT_MTU: u16 = 1420;
/// Timer period of the protocol.
const TIMER: Duration = Duration::from_millis(250);
/// Largest packet.
const BUF: usize = 65_536;

/// Why a tunnel could not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunnelError {
    /// No TUN device in this kernel.
    NoTun,
    /// The server's name could not be resolved.
    Resolve,
    /// The kernel refused (interface, address, route, socket).
    Kernel(NetError),
}

impl std::fmt::Display for TunnelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TunnelError::NoTun => f.write_str("no tun device"),
            TunnelError::Resolve => f.write_str("server name not resolved"),
            TunnelError::Kernel(e) => write!(f, "kernel: {e}"),
        }
    }
}

impl std::error::Error for TunnelError {}

impl From<NetError> for TunnelError {
    fn from(e: NetError) -> Self {
        match e {
            NetError::Unavailable => TunnelError::NoTun,
            other => TunnelError::Kernel(other),
        }
    }
}

fn io(e: std::io::Error) -> TunnelError {
    TunnelError::Kernel(NetError::Kernel(e.raw_os_error().unwrap_or(5)))
}

/// What the tunnel reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Time since the last handshake.
    pub handshake_age: Option<Duration>,
    /// Bytes received.
    pub rx: u64,
    /// Bytes sent.
    pub tx: u64,
}

/// A running tunnel; stopped (interface removed) when dropped.
pub struct Tunnel {
    stop: Arc<AtomicBool>,
    wake: UnixStream,
    stats: Arc<Mutex<Stats>>,
    thread: Option<std::thread::JoinHandle<()>>,
    server_route: Option<Route>,
}

/// `ip` lies in `net`.
pub fn contains(net: &Net, ip: &IpAddr) -> bool {
    let (base, prefix) = net;
    let bits = |a: &IpAddr| -> (u128, u32) {
        match a {
            IpAddr::V4(v) => (u128::from(u32::from(*v)) << 96, 32),
            IpAddr::V6(v) => (u128::from(*v), 128),
        }
    };
    let ((b, bl), (i, il)) = (bits(base), bits(ip));
    if bl != il {
        return false;
    }
    let p = u32::from(*prefix).min(bl);
    p == 0 || (b ^ i) >> (128 - p) == 0
}

fn resolve(conf: &TunnelConf) -> Result<SocketAddr, TunnelError> {
    let list: Vec<SocketAddr> = (conf.endpoint.host.as_str(), conf.endpoint.port)
        .to_socket_addrs()
        .map_err(|_| TunnelError::Resolve)?
        .collect();
    list.iter()
        .find(|a| a.is_ipv4())
        .or(list.first())
        .copied()
        .ok_or(TunnelError::Resolve)
}

fn halves(v6: bool) -> [Net; 2] {
    if v6 {
        [
            (IpAddr::from([0u16; 8]), 1),
            (IpAddr::from([0x8000u16, 0, 0, 0, 0, 0, 0, 0]), 1),
        ]
    } else {
        [
            (IpAddr::from([0u8; 4]), 1),
            (IpAddr::from([128u8, 0, 0, 0]), 1),
        ]
    }
}

/// Addresses, MTU, up, routes; the server's host route when needed.
fn configure(
    name: &str,
    conf: &TunnelConf,
    server: &SocketAddr,
) -> Result<Option<Route>, TunnelError> {
    let mut rtnl = Rtnl::open()?;
    let link = rtnl.link(name)?;
    rtnl.set_mtu(link.index, u32::from(conf.mtu.unwrap_or(DEFAULT_MTU)))?;
    for (a, p) in &conf.addresses {
        rtnl.add_address(link.index, *a, *p)?;
    }
    rtnl.set_up(link.index, true)?;
    let mut server_route = None;
    let route = |dst: IpAddr, prefix: u8| Route {
        dst,
        prefix,
        gateway: None,
        oif: link.index,
        metric: 0,
        origin: Origin::Static,
    };
    for (net, prefix) in &conf.allowed {
        if *prefix > 0 {
            rtnl.add_route(&route(*net, *prefix))?;
            continue;
        }
        let v6 = net.is_ipv6();
        if v6 == server.is_ipv6() && server_route.is_none() {
            let old = rtnl
                .routes(v6)?
                .into_iter()
                .find(|r| r.prefix == 0 && r.oif != link.index);
            if let Some(old) = old {
                let host = Route {
                    dst: server.ip(),
                    prefix: if v6 { 128 } else { 32 },
                    metric: 0,
                    ..old
                };
                rtnl.add_route(&host)?;
                server_route = Some(host);
            }
        }
        for (dst, p) in halves(v6) {
            rtnl.add_route(&route(dst, p))?;
        }
    }
    Ok(server_route)
}

impl Tunnel {
    /// Starts the tunnel of `conf` with the TUN device below `root`.
    pub fn start(root: &Path, conf: &TunnelConf, index: u32) -> Result<Tunnel, TunnelError> {
        let server = resolve(conf)?;
        let bind: SocketAddr = if server.is_ipv6() {
            (
                std::net::Ipv6Addr::UNSPECIFIED,
                conf.listen_port.unwrap_or(0),
            )
                .into()
        } else {
            (
                std::net::Ipv4Addr::UNSPECIFIED,
                conf.listen_port.unwrap_or(0),
            )
                .into()
        };
        let tun = Tun::open(root, IFNAME)?;
        let udp = UdpSocket::bind(bind).map_err(io)?;
        udp.set_nonblocking(true).map_err(io)?;
        let server_route = configure(tun.name(), conf, &server)?;
        let tunn = Tunn::new(
            StaticSecret::from(conf.private_key.0),
            PublicKey::from(conf.peer_key),
            conf.preshared.as_ref().map(|k| k.0),
            conf.keepalive,
            index,
            None,
        );
        let (wake, wake_rx) = UnixStream::pair().map_err(io)?;
        wake_rx.set_nonblocking(true).map_err(io)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(Mutex::new(Stats::default()));
        let mut engine = Engine {
            tun,
            udp,
            tunn,
            peer: server,
            allowed: conf.allowed.clone(),
            wake: wake_rx,
            stop: stop.clone(),
            stats: stats.clone(),
        };
        let thread = std::thread::Builder::new()
            .name("wireguard".into())
            .spawn(move || engine.run())
            .map_err(io)?;
        Ok(Tunnel {
            stop,
            wake,
            stats,
            thread: Some(thread),
            server_route,
        })
    }

    /// What the tunnel reports now.
    pub fn stats(&self) -> Stats {
        self.stats.lock().map(|s| *s).unwrap_or_default()
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::io::Write::write(&mut self.wake, &[1]);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if let (Some(r), Ok(mut rtnl)) = (self.server_route.take(), Rtnl::open()) {
            let _ = rtnl.del_route(&r);
        }
    }
}

struct Engine {
    tun: Tun,
    udp: UdpSocket,
    tunn: Tunn,
    peer: SocketAddr,
    allowed: Vec<Net>,
    wake: UnixStream,
    stop: Arc<AtomicBool>,
    stats: Arc<Mutex<Stats>>,
}

impl Engine {
    fn run(&mut self) {
        let mut a = vec![0u8; BUF];
        let mut b = vec![0u8; BUF + 64];
        if let TunnResult::WriteToNetwork(p) = self.tunn.format_handshake_initiation(&mut b, false)
        {
            let _ = self.udp.send_to(p, self.peer);
        }
        let mut timer = Instant::now();
        while !self.stop.load(Ordering::SeqCst) {
            self.wait();
            self.outgoing(&mut a, &mut b);
            self.incoming(&mut a, &mut b);
            if timer.elapsed() >= TIMER {
                timer = Instant::now();
                if let TunnResult::WriteToNetwork(p) = self.tunn.update_timers(&mut b) {
                    let _ = self.udp.send_to(p, self.peer);
                }
                let (age, tx, rx, _, _) = self.tunn.stats();
                if let Ok(mut s) = self.stats.lock() {
                    *s = Stats {
                        handshake_age: age,
                        rx: rx as u64,
                        tx: tx as u64,
                    };
                }
            }
        }
    }

    fn wait(&mut self) {
        let t = Timespec {
            tv_sec: 0,
            tv_nsec: TIMER.as_nanos() as i64,
        };
        let mut fds = [
            PollFd::new(&self.wake, PollFlags::IN),
            PollFd::from_borrowed_fd(self.tun.fd(), PollFlags::IN),
            PollFd::from_borrowed_fd(self.udp.as_fd(), PollFlags::IN),
        ];
        let _ = poll(&mut fds, Some(&t));
        let mut drain = [0u8; 16];
        while matches!(self.wake.read(&mut drain), Ok(n) if n > 0) {}
    }

    fn outgoing(&mut self, a: &mut [u8], b: &mut [u8]) {
        for _ in 0..256 {
            let Ok(Some(n)) = self.tun.read(a) else {
                return;
            };
            if let TunnResult::WriteToNetwork(p) = self.tunn.encapsulate(&a[..n], b) {
                let _ = self.udp.send_to(p, self.peer);
            }
        }
    }

    fn incoming(&mut self, a: &mut [u8], b: &mut [u8]) {
        for _ in 0..256 {
            let Ok((n, from)) = self.udp.recv_from(a) else {
                return;
            };
            let mut first = true;
            loop {
                let data: &[u8] = if first { &a[..n] } else { &[] };
                let src = first.then_some(from.ip());
                first = false;
                match self.tunn.decapsulate(src, data, b) {
                    TunnResult::WriteToNetwork(p) => {
                        let _ = self.udp.send_to(p, from);
                    }
                    TunnResult::WriteToTunnelV4(p, src) => {
                        self.deliver(p, IpAddr::V4(src), from);
                        break;
                    }
                    TunnResult::WriteToTunnelV6(p, src) => {
                        self.deliver(p, IpAddr::V6(src), from);
                        break;
                    }
                    TunnResult::Done | TunnResult::Err(_) => break,
                }
            }
        }
    }

    fn deliver(&mut self, packet: &[u8], inner: IpAddr, from: SocketAddr) {
        // An authenticated packet: the server may have moved (roaming).
        self.peer = from;
        if self.allowed.iter().any(|n| contains(n, &inner)) {
            let _ = self.tun.write(packet);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn networks_and_errors() {
        let n = |s: &str, p| (s.parse::<IpAddr>().unwrap(), p);
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        assert!(contains(&n("10.0.0.0", 8), &ip("10.200.1.1")));
        assert!(!contains(&n("10.0.0.0", 8), &ip("11.0.0.1")));
        assert!(contains(&n("0.0.0.0", 0), &ip("8.8.8.8")));
        assert!(!contains(&n("0.0.0.0", 0), &ip("::1")));
        assert!(contains(&n("fd00::", 64), &ip("fd00::5")));
        assert!(!contains(&n("fd00::", 64), &ip("fd01::5")));
        assert!(contains(&n("10.9.0.2", 32), &ip("10.9.0.2")));
        assert!(!contains(&n("10.9.0.2", 32), &ip("10.9.0.3")));
        assert_eq!(halves(false)[1], n("128.0.0.0", 1));
        assert_eq!(halves(true)[1], n("8000::", 1));
        assert_eq!(TunnelError::from(NetError::Unavailable), TunnelError::NoTun);
        assert!(!TunnelError::Resolve.to_string().is_empty());
        assert!(!TunnelError::Kernel(NetError::Timeout)
            .to_string()
            .is_empty());
    }
}
