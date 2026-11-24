//! Two real WireGuard tunnels of this service (boringtun over TUN) in two
//! network namespaces joined by a veth pair: one side routes everything
//! through the tunnel (split default route, server host route through the
//! old gateway), UDP datagrams cross the tunnel and come back. Needs root
//! and `ip`; without them the test says so and passes.

use std::net::{IpAddr, UdpSocket};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};
use ultar_hal_netlink::rtnl::Rtnl;
use ultar_vpnd::conf::{Endpoint, Key, TunnelConf};
use ultar_vpnd::tunnel::{Stats, Tunnel};

fn ip(args: &[&str]) {
    assert!(
        Command::new("ip").args(args).status().unwrap().success(),
        "ip {args:?}"
    );
}

struct Lab(Vec<String>);

impl Drop for Lab {
    fn drop(&mut self) {
        for ns in &self.0 {
            let _ = Command::new("ip").args(["netns", "del", ns]).status();
        }
    }
}

fn enter(ns: &str) {
    let f = std::fs::File::open(format!("/run/netns/{ns}")).unwrap();
    rustix::thread::move_into_link_name_space(
        std::os::fd::AsFd::as_fd(&f),
        Some(rustix::thread::LinkNameSpaceType::Network),
    )
    .unwrap();
}

fn public(secret: [u8; 32]) -> [u8; 32] {
    boringtun::x25519::PublicKey::from(&boringtun::x25519::StaticSecret::from(secret)).to_bytes()
}

fn conf(
    own: [u8; 32],
    peer: [u8; 32],
    addr: &str,
    allowed: (&str, u8),
    to: &str,
    port: u16,
    listen: u16,
) -> TunnelConf {
    TunnelConf {
        private_key: Key(own),
        addresses: vec![(addr.parse().unwrap(), 24)],
        dns: vec!["10.9.0.53".parse().unwrap()],
        mtu: None,
        listen_port: Some(listen),
        peer_key: public(peer),
        preshared: Some(Key([7; 32])),
        allowed: vec![(allowed.0.parse().unwrap(), allowed.1)],
        endpoint: Endpoint {
            host: to.into(),
            port,
        },
        keepalive: Some(5),
    }
}

/// Two namespaces joined by veth: A 10.78.0.1 (default route via B), B 10.78.0.2.
fn lab(id: u32) -> (Lab, String, String) {
    let (a, b) = (format!("ultar-wg-a{id}"), format!("ultar-wg-b{id}"));
    let lab = Lab(vec![a.clone(), b.clone()]);
    let (va, vb) = (format!("uwa{id}"), format!("uwb{id}"));
    ip(&["netns", "add", &a]);
    ip(&["netns", "add", &b]);
    ip(&[
        "link", "add", &va, "netns", &a, "type", "veth", "peer", "name", &vb, "netns", &b,
    ]);
    ip(&["-n", &a, "addr", "add", "10.78.0.1/24", "dev", &va]);
    ip(&["-n", &b, "addr", "add", "10.78.0.2/24", "dev", &vb]);
    for (ns, dev) in [(&a, &va), (&b, &vb)] {
        ip(&["-n", ns, "link", "set", dev, "up"]);
        ip(&["-n", ns, "link", "set", "lo", "up"]);
    }
    ip(&["-n", &a, "route", "add", "default", "via", "10.78.0.2"]);
    (lab, a, b)
}

/// Side B: its tunnel and a UDP echo on 10.9.0.2:7777 until `stop`.
fn server(ns: String, c: TunnelConf, ready: Sender<()>, stop: Receiver<()>) {
    enter(&ns);
    let _t = Tunnel::start(std::path::Path::new("/"), &c, 2).unwrap();
    let echo = UdpSocket::bind("10.9.0.2:7777").unwrap();
    echo.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    ready.send(()).unwrap();
    let mut buf = [0u8; 64];
    while stop.try_recv().is_err() {
        if let Ok((n, from)) = echo.recv_from(&mut buf) {
            assert_eq!(
                from.ip(),
                "10.9.0.1".parse::<IpAddr>().unwrap(),
                "inner source"
            );
            echo.send_to(&buf[..n], from).unwrap();
        }
    }
}

fn has(routes: &[ultar_hal_netlink::rtnl::Route], dst: &str, p: u8) -> bool {
    routes
        .iter()
        .any(|r| r.dst == dst.parse::<IpAddr>().unwrap() && r.prefix == p)
}

/// Side A: full tunnel, datagrams to B's echo; (echoed, stats, server route left).
fn client(ns: String, c: TunnelConf) -> (bool, Stats, bool) {
    enter(&ns);
    let t = Tunnel::start(std::path::Path::new("/"), &c, 1).unwrap();
    let mut rtnl = Rtnl::open().unwrap();
    let routes = rtnl.routes(false).unwrap();
    assert!(
        has(&routes, "0.0.0.0", 1) && has(&routes, "128.0.0.0", 1),
        "split default route"
    );
    assert!(has(&routes, "10.78.0.2", 32), "server keeps its way out");
    let s = UdpSocket::bind("0.0.0.0:0").unwrap();
    s.set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let start = Instant::now();
    let mut buf = [0u8; 64];
    let mut echoed = false;
    while start.elapsed() < Duration::from_secs(20) && !echoed {
        s.send_to(b"through the tunnel", "10.9.0.2:7777").unwrap();
        echoed =
            matches!(s.recv_from(&mut buf), Ok((18, _))) && &buf[..18] == b"through the tunnel";
    }
    std::thread::sleep(Duration::from_millis(600));
    let stats = t.stats();
    drop(t);
    (
        echoed,
        stats,
        has(&rtnl.routes(false).unwrap(), "10.78.0.2", 32),
    )
}

#[test]
fn two_peers_talk_through_the_tunnel() {
    let root = ultar_service::effective_uid() == 0;
    let have_ip = Command::new("ip")
        .arg("-V")
        .stdout(Stdio::null())
        .status()
        .is_ok();
    if !root || !have_ip || !std::path::Path::new("/dev/net/tun").exists() {
        eprintln!("skipped: needs root, ip and /dev/net/tun");
        return;
    }
    let (_lab, a, b) = lab(std::process::id() % 100_000);
    let (sa, sb) = ([0x11u8; 32], [0x22u8; 32]);
    let ca = conf(
        sa,
        sb,
        "10.9.0.1",
        ("0.0.0.0", 0),
        "10.78.0.2",
        51820,
        51821,
    );
    let cb = conf(
        sb,
        sa,
        "10.9.0.2",
        ("10.9.0.1", 32),
        "10.78.0.1",
        51821,
        51820,
    );
    let (stop_b, stopped_b) = channel::<()>();
    let (ready_b, b_ready) = channel::<()>();
    let side_b = std::thread::spawn(move || server(b, cb, ready_b, stopped_b));
    b_ready.recv().unwrap();
    let (echoed, stats, host_left) = std::thread::spawn(move || client(a, ca)).join().unwrap();
    stop_b.send(()).unwrap();
    side_b.join().unwrap();
    assert!(echoed, "datagram crossed the tunnel and came back");
    assert!(
        stats.handshake_age.is_some() && stats.rx > 0 && stats.tx > 0,
        "{stats:?}"
    );
    assert!(!host_left, "server route removed with the tunnel");
}
