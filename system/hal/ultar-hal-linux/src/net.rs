//! Network interfaces as the kernel lists them (`/sys/class/net`, Linux layout): wired links
//! with their speed, Wi-Fi interfaces, whether a link is up, the hardware address and driver.
//! The Ultar kernel adds `ultar_ipv4` per interface: the IPv4 state (DHCP lease or fixed
//! address, router, DNS) as `key=value` lines, and the place to switch it (`dhcp`, `renew`,
//! `static <a.b.c.d>/<nn> [router|-] [dns[,dns2]]`). Linux has no such file: there the
//! address state is `None` and switching it answers [`HalError::NotSupported`].

use crate::sysfs::{entries, number, text};
use std::net::Ipv4Addr;
use std::path::PathBuf;
use ultar_hal::HalError;

/// The IPv4 state of an interface as the Ultar kernel reports it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Ipv4State {
    /// A fixed address (`mode=static`) instead of DHCP.
    pub fixed: bool,
    /// The kernel's word for the state (`bound`, `selecting`, `renewing`, `static`, `nolink` …).
    pub state: String,
    /// The interface carries the default route.
    pub active: bool,
    /// Address with prefix (`10.0.2.15/24`).
    pub address: Option<String>,
    /// Default router.
    pub gateway: Option<String>,
    /// Name servers.
    pub dns: Vec<String>,
    /// Lease length in seconds (DHCP).
    pub lease_secs: Option<u32>,
}

impl Ipv4State {
    /// Reads the `key=value` text of `ultar_ipv4`.
    pub fn parse(text: &str) -> Ipv4State {
        let mut s = Ipv4State::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim();
            match k.trim() {
                "mode" => s.fixed = v == "static",
                "state" => s.state = v.to_owned(),
                "active" => s.active = v == "1",
                "address" if !v.is_empty() => s.address = Some(v.to_owned()),
                "gateway" if !v.is_empty() => s.gateway = Some(v.to_owned()),
                "dns" => s.dns = v.split_whitespace().map(str::to_owned).collect(),
                "lease" => s.lease_secs = v.parse().ok(),
                _ => {}
            }
        }
        s
    }
}

/// One interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    /// Name (`eth0`, `wlan0`).
    pub name: String,
    /// A Wi-Fi interface.
    pub wireless: bool,
    /// The link is up and carries traffic.
    pub up: bool,
    /// Speed in Mbit/s (wired; `None` when the driver does not say).
    pub speed_mbit: Option<u32>,
    /// Hardware address (`aa:bb:cc:dd:ee:ff`).
    pub mac: Option<String>,
    /// The kernel driver (Ultar kernel: `ultar_driver`; Linux: the `device/driver` link).
    pub driver: Option<String>,
    /// The IPv4 state (Ultar kernel only).
    pub ipv4: Option<Ipv4State>,
}

/// The interfaces below a root directory.
#[derive(Clone, Debug)]
pub struct Interfaces {
    root: PathBuf,
}

/// `ARPHRD_ETHER`: Ethernet and Wi-Fi.
const ETHER: i64 = 1;

/// A fixed IPv4 configuration as the kernel takes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticIpv4 {
    /// Address.
    pub address: Ipv4Addr,
    /// Prefix length, 1–32.
    pub prefix: u8,
    /// Default router.
    pub gateway: Option<Ipv4Addr>,
    /// Name servers (at most two are used).
    pub dns: Vec<Ipv4Addr>,
}

/// What `Interfaces::set_ipv4` asks the kernel for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ipv4Request {
    /// Address by DHCP.
    Dhcp,
    /// Ask the DHCP server again.
    Renew,
    /// A fixed address.
    Static(StaticIpv4),
}

impl Ipv4Request {
    /// The command line written to `ultar_ipv4`.
    pub fn command(&self) -> String {
        match self {
            Ipv4Request::Dhcp => "dhcp".into(),
            Ipv4Request::Renew => "renew".into(),
            Ipv4Request::Static(s) => {
                let mut c = format!("static {}/{}", s.address, s.prefix);
                c.push(' ');
                match s.gateway {
                    Some(g) => c.push_str(&g.to_string()),
                    None => c.push('-'),
                }
                let dns: Vec<String> = s.dns.iter().take(2).map(|d| d.to_string()).collect();
                if !dns.is_empty() {
                    c.push(' ');
                    c.push_str(&dns.join(","));
                }
                c
            }
        }
    }
}

impl Interfaces {
    /// Interfaces below `root` (`/` on a device).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn dir(&self) -> PathBuf {
        self.root.join("sys/class/net")
    }

    /// Every Ethernet or Wi-Fi interface (loopback, tunnels and bridges left out).
    pub fn list(&self) -> Vec<Interface> {
        entries(&self.dir(), 128)
            .into_iter()
            .filter(|p| number(&p.join("type")) == Some(ETHER) && !p.join("bridge").exists())
            .filter_map(|p| {
                let name = p.file_name()?.to_str()?.to_owned();
                let wireless = p.join("wireless").exists() || p.join("phy80211").exists();
                let up = text(&p.join("operstate")).as_deref() == Some("up")
                    && number(&p.join("carrier")) == Some(1);
                let speed_mbit = number(&p.join("speed"))
                    .filter(|s| *s > 0)
                    .and_then(|s| u32::try_from(s).ok());
                let mac = text(&p.join("address")).filter(|m| m.len() == 17);
                let driver = text(&p.join("ultar_driver"))
                    .filter(|d| !d.is_empty() && d != "-")
                    .or_else(|| {
                        let link = std::fs::read_link(p.join("device/driver")).ok()?;
                        Some(link.file_name()?.to_str()?.to_owned())
                    });
                let ipv4 = text(&p.join("ultar_ipv4")).map(|t| Ipv4State::parse(&t));
                Some(Interface {
                    name,
                    wireless,
                    up,
                    speed_mbit,
                    mac,
                    driver,
                    ipv4,
                })
            })
            .collect()
    }

    /// Switches the IPv4 configuration of interface `name` (Ultar kernel; needs the
    /// network right).
    pub fn set_ipv4(&self, name: &str, request: &Ipv4Request) -> Result<(), HalError> {
        if name.is_empty() || name.contains('/') || name.starts_with('.') {
            return Err(HalError::InvalidArgument);
        }
        let iface = self.dir().join(name);
        if !iface.exists() {
            return Err(HalError::NotFound);
        }
        let file = iface.join("ultar_ipv4");
        if !file.exists() {
            return Err(HalError::NotSupported);
        }
        std::fs::write(&file, request.command()).map_err(|e| match e.raw_os_error() {
            Some(22) => HalError::InvalidArgument,
            Some(1) | Some(13) => HalError::PermissionDenied,
            Some(n) => HalError::Io(n),
            None => HalError::Io(5),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &std::path::Path, path: &str, value: &str) {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, value).unwrap();
    }

    #[test]
    fn wired_and_wireless_are_told_apart() {
        let root = std::env::temp_dir().join(format!("hal-net-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (n, t) in [("lo", "772"), ("eth0", "1"), ("wlan0", "1"), ("br0", "1")] {
            put(&root, &format!("sys/class/net/{n}/type"), t);
        }
        put(&root, "sys/class/net/eth0/operstate", "up\n");
        put(&root, "sys/class/net/eth0/carrier", "1\n");
        put(&root, "sys/class/net/eth0/speed", "1000\n");
        put(&root, "sys/class/net/eth0/address", "52:54:00:12:34:56\n");
        put(&root, "sys/class/net/eth0/ultar_driver", "vmxnet3\n");
        put(
            &root,
            "sys/class/net/eth0/ultar_ipv4",
            "mode=dhcp\nstate=bound\nactive=1\naddress=10.0.2.15/24\ngateway=10.0.2.2\ndns=10.0.2.3 10.0.2.4\nlease=86400\n",
        );
        put(&root, "sys/class/net/wlan0/operstate", "down\n");
        put(&root, "sys/class/net/wlan0/speed", "-1\n");
        std::fs::create_dir_all(root.join("sys/class/net/wlan0/wireless")).unwrap();
        std::fs::create_dir_all(root.join("sys/class/net/br0/bridge")).unwrap();
        let list = Interfaces::new(&root).list();
        assert_eq!(list.len(), 2);
        assert_eq!(
            list[0],
            Interface {
                name: "eth0".into(),
                wireless: false,
                up: true,
                speed_mbit: Some(1000),
                mac: Some("52:54:00:12:34:56".into()),
                driver: Some("vmxnet3".into()),
                ipv4: Some(Ipv4State {
                    fixed: false,
                    state: "bound".into(),
                    active: true,
                    address: Some("10.0.2.15/24".into()),
                    gateway: Some("10.0.2.2".into()),
                    dns: vec!["10.0.2.3".into(), "10.0.2.4".into()],
                    lease_secs: Some(86400),
                }),
            }
        );
        assert!(list[1].wireless && !list[1].up && list[1].speed_mbit.is_none());
        assert!(list[1].ipv4.is_none() && list[1].mac.is_none());
        assert!(Interfaces::new(root.join("none")).list().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ipv4_requests_are_written_as_the_kernel_reads_them() {
        let root = std::env::temp_dir().join(format!("hal-net-set-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        put(&root, "sys/class/net/eth0/type", "1");
        put(&root, "sys/class/net/eth0/ultar_ipv4", "mode=dhcp\n");
        put(&root, "sys/class/net/eth1/type", "1");
        let i = Interfaces::new(&root);
        let fixed = Ipv4Request::Static(StaticIpv4 {
            address: Ipv4Addr::new(192, 168, 1, 20),
            prefix: 24,
            gateway: Some(Ipv4Addr::new(192, 168, 1, 1)),
            dns: vec![
                Ipv4Addr::new(1, 1, 1, 1),
                Ipv4Addr::new(9, 9, 9, 9),
                Ipv4Addr::new(8, 8, 8, 8),
            ],
        });
        i.set_ipv4("eth0", &fixed).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("sys/class/net/eth0/ultar_ipv4")).unwrap(),
            "static 192.168.1.20/24 192.168.1.1 1.1.1.1,9.9.9.9"
        );
        let local = Ipv4Request::Static(StaticIpv4 {
            address: Ipv4Addr::new(10, 1, 1, 2),
            prefix: 8,
            gateway: None,
            dns: vec![],
        });
        assert_eq!(local.command(), "static 10.1.1.2/8 -");
        assert_eq!(
            i.set_ipv4("eth1", &Ipv4Request::Dhcp),
            Err(HalError::NotSupported)
        );
        assert_eq!(
            i.set_ipv4("eth9", &Ipv4Request::Renew),
            Err(HalError::NotFound)
        );
        assert_eq!(
            i.set_ipv4("../x", &Ipv4Request::Renew),
            Err(HalError::InvalidArgument)
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
