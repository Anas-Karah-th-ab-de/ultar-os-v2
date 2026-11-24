//! The DHCP client of the service against a real DHCP server (dnsmasq) over a
//! veth pair between two network namespaces of the Linux kernel: real
//! sockets (UDP 68 bound to the interface, broadcast), real addresses and
//! routes through rtnetlink. Needs root, `ip` and `dnsmasq`; without them the
//! test says so and passes (the gate runs it as root).

use std::net::{IpAddr, Ipv4Addr};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use ultar_hal_netlink::rtnl::Rtnl;
use ultar_netd::wifi::driver::{IpLayer, LinuxIp};
use ultar_netproto::dhcp::{Action, Client, Lease};

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn ip(args: &[&str]) {
    let ok = Command::new("ip").args(args).status().unwrap().success();
    assert!(ok, "ip {args:?}");
}

struct Lab {
    server_ns: String,
    client_ns: String,
    dnsmasq: Option<Child>,
    dir: std::path::PathBuf,
}

impl Drop for Lab {
    fn drop(&mut self) {
        if let Some(mut d) = self.dnsmasq.take() {
            let _ = d.kill();
            let _ = d.wait();
        }
        for ns in [&self.server_ns, &self.client_ns] {
            let _ = Command::new("ip").args(["netns", "del", ns]).status();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn lab(veth_c: &str, veth_s: &str) -> Lab {
    let id = std::process::id();
    let dir = std::env::temp_dir().join(format!("netd-dhcp-{id}"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut lab = Lab {
        server_ns: format!("ultar-dhcp-s{id}"),
        client_ns: format!("ultar-dhcp-c{id}"),
        dnsmasq: None,
        dir,
    };
    ip(&["netns", "add", &lab.server_ns]);
    ip(&["netns", "add", &lab.client_ns]);
    ip(&[
        "link",
        "add",
        veth_c,
        "netns",
        &lab.client_ns,
        "type",
        "veth",
        "peer",
        "name",
        veth_s,
        "netns",
        &lab.server_ns,
    ]);
    ip(&[
        "-n",
        &lab.server_ns,
        "addr",
        "add",
        "10.77.0.1/24",
        "dev",
        veth_s,
    ]);
    ip(&["-n", &lab.server_ns, "link", "set", veth_s, "up"]);
    ip(&["-n", &lab.client_ns, "link", "set", veth_c, "up"]);
    lab.dnsmasq = Some(dnsmasq(&lab, veth_s));
    lab
}

/// dnsmasq on the server side: 10.77.0.50–60, router .1, DNS .53.
fn dnsmasq(lab: &Lab, veth_s: &str) -> Child {
    let leases = lab.dir.join("leases");
    Command::new("ip")
        .args([
            "netns",
            "exec",
            &lab.server_ns,
            "dnsmasq",
            "--no-daemon",
            "--conf-file=/dev/null",
        ])
        .arg(format!("--interface={veth_s}"))
        .args([
            "--bind-interfaces",
            "--port=0",
            "--dhcp-range=10.77.0.50,10.77.0.60,255.255.255.0,1h",
            "--dhcp-option=option:router,10.77.0.1",
            "--dhcp-option=option:dns-server,10.77.0.53",
        ])
        .arg(format!("--dhcp-leasefile={}", leases.display()))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// Runs the client in the client namespace until it is configured.
fn client_in(ns: &str, veth: &str) -> (Lease, bool, bool) {
    let ns_file = std::fs::File::open(format!("/run/netns/{ns}")).unwrap();
    rustix::thread::move_into_link_name_space(
        std::os::fd::AsFd::as_fd(&ns_file),
        Some(rustix::thread::LinkNameSpaceType::Network),
    )
    .unwrap();
    let mut rtnl = Rtnl::open().unwrap();
    let link = rtnl.link(veth).unwrap();
    let mut layer = LinuxIp::default();
    layer.open(veth).unwrap();
    let start = Instant::now();
    let now = || start.elapsed().as_millis() as u64;
    let mut client = Client::new(link.mac.unwrap(), 0x5eed_0001, now());
    let mut buf = [0u8; 1500];
    while start.elapsed() < Duration::from_secs(20) {
        let mut actions = client.poll(now());
        if let Ok(Some(n)) = layer.recv(&mut buf) {
            actions.extend(client.receive(&buf[..n], now()));
        }
        for a in actions {
            match a {
                Action::Broadcast(b) => layer.send(Ipv4Addr::BROADCAST, &b).unwrap(),
                Action::Unicast(to, b) => layer.send(to, &b).unwrap(),
                Action::Configure(lease) => {
                    layer.configure(veth, &lease).unwrap();
                    let routed = rtnl.routes(false).unwrap().iter().any(|r| {
                        r.prefix == 0 && r.gateway == Some(IpAddr::V4(Ipv4Addr::new(10, 77, 0, 1)))
                    });
                    layer.deconfigure(veth).unwrap();
                    let gone = !rtnl
                        .routes(false)
                        .unwrap()
                        .iter()
                        .any(|r| r.oif == link.index && r.prefix == 0);
                    return (lease, routed, gone);
                }
                Action::Deconfigure => {}
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("no lease from dnsmasq");
}

#[test]
fn a_real_dhcp_server_gives_an_address_route_and_dns() {
    let root = ultar_service::effective_uid() == 0;
    if !root || !have("ip") || !have("dnsmasq") || !std::path::Path::new("/run").exists() {
        eprintln!("skipped: needs root, ip and dnsmasq");
        return;
    }
    let id = std::process::id() % 100_000;
    let (vc, vs) = (format!("uvc{id}"), format!("uvs{id}"));
    let lab = lab(&vc, &vs);
    let ns = lab.client_ns.clone();
    let (lease, routed, gone) = std::thread::spawn(move || client_in(&ns, &vc))
        .join()
        .unwrap();
    let last = lease.address.octets()[3];
    assert_eq!(&lease.address.octets()[..3], &[10, 77, 0]);
    assert!((50..=60).contains(&last), "{lease:?}");
    assert_eq!(lease.prefix, 24);
    assert_eq!(lease.router, Some(Ipv4Addr::new(10, 77, 0, 1)));
    assert_eq!(lease.dns, vec![Ipv4Addr::new(10, 77, 0, 53)]);
    assert!(routed, "default route via the router");
    assert!(gone, "deconfigure removes address and route");
    drop(lab);
}
