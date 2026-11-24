//! The interfaces as the settings and the shell see them: every wired interface and the Wi-Fi
//! interface as their own [`NetLink`] (from the kernel's interface list and, for Wi-Fi, the
//! supplicant's connection), and fixed IPv4 addresses kept across restarts.

use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use ultar_bus::messages::net::{
    AddressMethod, Ipv4Config, LinkKind, LinkState, NetLink, WifiLink, WifiPhase,
};
use ultar_hal_linux::net::{Interface, Ipv4Request, StaticIpv4};

/// A wired interface as a [`NetLink`].
pub fn ethernet(i: &Interface) -> NetLink {
    let v4 = i.ipv4.as_ref();
    let address = v4.and_then(|v| v.address.clone());
    let state = if !i.up {
        LinkState::NoCarrier
    } else if address.is_some() {
        LinkState::Connected
    } else if v4.is_none() {
        // Linux: the address is not read here; a link that is up counts as connected
        LinkState::Connected
    } else {
        LinkState::Connecting
    };
    let method = match v4 {
        Some(v) if v.fixed => AddressMethod::Static,
        Some(_) => AddressMethod::Dhcp,
        None => AddressMethod::Unknown,
    };
    NetLink {
        name: i.name.clone(),
        kind: LinkKind::Ethernet,
        mac: i.mac.clone().unwrap_or_default(),
        driver: i.driver.clone(),
        state,
        speed_mbit: if i.up { i.speed_mbit } else { None },
        method,
        address,
        gateway: v4.and_then(|v| v.gateway.clone()),
        dns: v4.map(|v| v.dns.clone()).unwrap_or_default(),
        lease_secs: v4.filter(|v| !v.fixed).and_then(|v| v.lease_secs),
        ipv6: Vec::new(),
    }
}

/// The Wi-Fi interface (`None`: no Wi-Fi adapter): the kernel's interface (name, MAC,
/// driver) and the supplicant's connection (address, router, DNS).
pub fn wifi(iface: Option<&Interface>, link: &WifiLink, radio_on: bool) -> Option<NetLink> {
    if iface.is_none() && link.phase == WifiPhase::NoDevice {
        return None;
    }
    let state = match link.phase {
        WifiPhase::Connected => LinkState::Connected,
        WifiPhase::Associating | WifiPhase::Authenticating | WifiPhase::ObtainingAddress => {
            LinkState::Connecting
        }
        WifiPhase::Off => LinkState::Off,
        _ if !radio_on => LinkState::Off,
        _ => LinkState::NoCarrier,
    };
    let connected = state == LinkState::Connected;
    Some(NetLink {
        name: iface.map(|i| i.name.clone()).unwrap_or_default(),
        kind: LinkKind::Wifi,
        mac: iface.and_then(|i| i.mac.clone()).unwrap_or_default(),
        driver: iface.and_then(|i| i.driver.clone()),
        state,
        speed_mbit: iface.filter(|_| connected).and_then(|i| i.speed_mbit),
        method: AddressMethod::Dhcp,
        address: link.address.clone().filter(|_| connected),
        gateway: link.router.clone().filter(|_| connected),
        dns: if connected {
            link.dns.clone()
        } else {
            Vec::new()
        },
        lease_secs: None,
        ipv6: Vec::new(),
    })
}

/// The kernel request for a configuration (`None`: not valid).
pub fn request(c: &Ipv4Config) -> Option<Ipv4Request> {
    Some(match c {
        Ipv4Config::Dhcp => Ipv4Request::Dhcp,
        Ipv4Config::Renew => Ipv4Request::Renew,
        Ipv4Config::Static {
            address,
            gateway,
            dns,
        } => {
            let (ip, prefix) = address.split_once('/')?;
            let address: Ipv4Addr = ip.parse().ok()?;
            let prefix: u8 = prefix.parse().ok().filter(|p| (1..=32).contains(p))?;
            if address.is_unspecified() || address.is_broadcast() || address.is_multicast() {
                return None;
            }
            let gateway = match gateway {
                Some(g) => Some(g.parse::<Ipv4Addr>().ok()?),
                None => None,
            };
            let dns = dns
                .iter()
                .map(|d| d.parse::<Ipv4Addr>().ok())
                .collect::<Option<Vec<_>>>()?;
            if dns.len() > 2 {
                return None;
            }
            Ipv4Request::Static(StaticIpv4 {
                address,
                prefix,
                gateway,
                dns,
            })
        }
    })
}

/// Fixed addresses wanted per interface (kept below the service's directory).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Fixed {
    /// Interface name and its configuration (only `Static` entries are kept).
    pub entries: Vec<(String, Ipv4Config)>,
}

impl Fixed {
    /// Notes the wish for `name`: a fixed address is kept, DHCP drops it, renewing changes
    /// nothing.
    pub fn note(&mut self, name: &str, c: &Ipv4Config) {
        match c {
            Ipv4Config::Renew => {}
            Ipv4Config::Dhcp => self.entries.retain(|(n, _)| n != name),
            Ipv4Config::Static { .. } => {
                self.entries.retain(|(n, _)| n != name);
                self.entries.push((name.to_owned(), c.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_hal_linux::net::Ipv4State;

    fn eth(up: bool, ipv4: Option<Ipv4State>) -> Interface {
        Interface {
            name: "eth0".into(),
            wireless: false,
            up,
            speed_mbit: Some(1000),
            mac: Some("52:54:00:12:34:56".into()),
            driver: Some("gen-intel-eth".into()),
            ipv4,
        }
    }

    #[test]
    fn wired_states() {
        let lease = Ipv4State {
            state: "bound".into(),
            active: true,
            address: Some("10.0.2.15/24".into()),
            gateway: Some("10.0.2.2".into()),
            dns: vec!["10.0.2.3".into()],
            lease_secs: Some(86400),
            ..Ipv4State::default()
        };
        let l = ethernet(&eth(true, Some(lease.clone())));
        assert_eq!(l.state, LinkState::Connected);
        assert_eq!(l.method, AddressMethod::Dhcp);
        assert_eq!(l.address.as_deref(), Some("10.0.2.15/24"));
        assert_eq!(l.lease_secs, Some(86400));
        assert_eq!(l.speed_mbit, Some(1000));
        let cable_out = ethernet(&eth(false, Some(lease.clone())));
        assert_eq!(cable_out.state, LinkState::NoCarrier);
        assert_eq!(cable_out.speed_mbit, None);
        let asking = ethernet(&eth(
            true,
            Some(Ipv4State {
                state: "selecting".into(),
                ..Ipv4State::default()
            }),
        ));
        assert_eq!(asking.state, LinkState::Connecting);
        let fixed = ethernet(&eth(
            true,
            Some(Ipv4State {
                fixed: true,
                ..lease
            }),
        ));
        assert_eq!(fixed.method, AddressMethod::Static);
        assert_eq!(fixed.lease_secs, None);
        assert_eq!(ethernet(&eth(true, None)).method, AddressMethod::Unknown);
    }

    #[test]
    fn no_wifi_adapter_is_none() {
        assert_eq!(wifi(None, &WifiLink::default(), true), None);
        let link = WifiLink {
            phase: WifiPhase::Connected,
            ssid: Some("home".into()),
            signal_percent: Some(60),
            address: Some("192.168.1.5/24".into()),
            router: Some("192.168.1.1".into()),
            dns: vec!["192.168.1.1".into()],
        };
        let w = wifi(None, &link, true).unwrap();
        assert_eq!(w.kind, LinkKind::Wifi);
        assert_eq!(w.state, LinkState::Connected);
        assert_eq!(w.gateway.as_deref(), Some("192.168.1.1"));
        let off = WifiLink {
            phase: WifiPhase::Off,
            ..link
        };
        let w = wifi(None, &off, false).unwrap();
        assert_eq!(w.state, LinkState::Off);
        assert!(w.address.is_none() && w.dns.is_empty());
    }

    #[test]
    fn requests_are_checked() {
        let ok = Ipv4Config::Static {
            address: "192.168.1.20/24".into(),
            gateway: Some("192.168.1.1".into()),
            dns: vec!["1.1.1.1".into()],
        };
        assert!(matches!(request(&ok), Some(Ipv4Request::Static(_))));
        for bad in [
            "192.168.1.300/24",
            "192.168.1.2",
            "192.168.1.2/33",
            "0.0.0.0/8",
        ] {
            let c = Ipv4Config::Static {
                address: bad.into(),
                gateway: None,
                dns: vec![],
            };
            assert_eq!(request(&c), None, "{bad}");
        }
        let three_dns = Ipv4Config::Static {
            address: "10.0.0.2/8".into(),
            gateway: None,
            dns: vec!["1.1.1.1".into(), "8.8.8.8".into(), "9.9.9.9".into()],
        };
        assert_eq!(request(&three_dns), None);
        let mut f = Fixed::default();
        f.note("eth0", &ok);
        f.note("eth0", &Ipv4Config::Renew);
        assert_eq!(f.entries.len(), 1);
        f.note("eth0", &Ipv4Config::Dhcp);
        assert!(f.entries.is_empty());
    }
}
