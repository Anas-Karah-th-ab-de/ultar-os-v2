//! rtnetlink: links (index, address, up, MTU), addresses and routes – what
//! DHCP and the VPN tunnel configure.

use crate::attr::{self, Builder, F_CREATE, F_DUMP, F_EXCL, F_REPLACE, F_REQUEST};
use crate::socket::{Socket, CHANGE, ROUTE};
use crate::NetError;
use std::net::IpAddr;

const RTM_NEWLINK: u16 = 16;
const RTM_GETLINK: u16 = 18;
const RTM_NEWADDR: u16 = 20;
const RTM_DELADDR: u16 = 21;
const RTM_GETADDR: u16 = 22;
const RTM_NEWROUTE: u16 = 24;
const RTM_DELROUTE: u16 = 25;
const RTM_GETROUTE: u16 = 26;
const IFLA_ADDRESS: u16 = 1;
const IFLA_IFNAME: u16 = 3;
const IFLA_MTU: u16 = 4;
const IFA_ADDRESS: u16 = 1;
const IFA_LOCAL: u16 = 2;
const RTA_DST: u16 = 1;
const RTA_OIF: u16 = 4;
const RTA_GATEWAY: u16 = 5;
const RTA_PRIORITY: u16 = 6;
const RTA_TABLE: u16 = 15;
const IFF_UP: u32 = 1;
const AF_INET: u8 = 2;
const AF_INET6: u8 = 10;
const RT_TABLE_MAIN: u8 = 254;
const RT_SCOPE_UNIVERSE: u8 = 0;
const RT_SCOPE_LINK: u8 = 253;
const RTN_UNICAST: u8 = 1;

/// Who configured a route (`rtm_protocol`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The DHCP client.
    Dhcp = 16,
    /// Static (the VPN tunnel).
    Static = 4,
}

/// A network interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// Index.
    pub index: u32,
    /// Hardware address (none for a tunnel).
    pub mac: Option<[u8; 6]>,
    /// Administratively up.
    pub up: bool,
    /// MTU.
    pub mtu: Option<u32>,
}

/// A route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    /// Destination network.
    pub dst: IpAddr,
    /// Its prefix length (0: default route).
    pub prefix: u8,
    /// Next hop.
    pub gateway: Option<IpAddr>,
    /// Outgoing interface.
    pub oif: u32,
    /// Metric.
    pub metric: u32,
    /// Who set it.
    pub origin: Origin,
}

fn family(a: &IpAddr) -> u8 {
    match a {
        IpAddr::V4(_) => AF_INET,
        IpAddr::V6(_) => AF_INET6,
    }
}

fn bytes(a: &IpAddr) -> Vec<u8> {
    match a {
        IpAddr::V4(v) => v.octets().to_vec(),
        IpAddr::V6(v) => v.octets().to_vec(),
    }
}

fn ip(b: &[u8]) -> Option<IpAddr> {
    match b.len() {
        4 => <[u8; 4]>::try_from(b).ok().map(IpAddr::from),
        16 => <[u8; 16]>::try_from(b).ok().map(IpAddr::from),
        _ => None,
    }
}

fn ifinfo(index: u32, flags: u32, change: u32) -> [u8; 16] {
    let mut m = [0u8; 16];
    m[4..8].copy_from_slice(&index.to_ne_bytes());
    m[8..12].copy_from_slice(&flags.to_ne_bytes());
    m[12..16].copy_from_slice(&change.to_ne_bytes());
    m
}

/// Reads a link of an `RTM_GETLINK` answer.
pub fn parse_link(payload: &[u8]) -> Option<Link> {
    if payload.len() < 16 {
        return None;
    }
    let a = &payload[16..];
    Some(Link {
        index: u32::from_ne_bytes(payload[4..8].try_into().ok()?),
        mac: attr::get_mac(a, IFLA_ADDRESS),
        up: u32::from_ne_bytes(payload[8..12].try_into().ok()?) & IFF_UP != 0,
        mtu: attr::get_u32(a, IFLA_MTU),
    })
}

/// Reads a route of an `RTM_GETROUTE` answer (main table only).
pub fn parse_route(payload: &[u8]) -> Option<Route> {
    if payload.len() < 12 {
        return None;
    }
    let a = &payload[12..];
    let table = attr::get_u32(a, RTA_TABLE).unwrap_or(u32::from(payload[4]));
    if table != u32::from(RT_TABLE_MAIN) || payload[7] != RTN_UNICAST {
        return None;
    }
    let unspecified = if payload[0] == AF_INET6 {
        IpAddr::from([0u8; 16])
    } else {
        IpAddr::from([0u8; 4])
    };
    Some(Route {
        dst: attr::find(a, RTA_DST).and_then(ip).unwrap_or(unspecified),
        prefix: payload[1],
        gateway: attr::find(a, RTA_GATEWAY).and_then(ip),
        oif: attr::get_u32(a, RTA_OIF)?,
        metric: attr::get_u32(a, RTA_PRIORITY).unwrap_or(0),
        origin: Origin::Static,
    })
}

/// rtnetlink on its own socket.
#[derive(Debug)]
pub struct Rtnl {
    socket: Socket,
}

impl Rtnl {
    /// Opens rtnetlink.
    pub fn open() -> Result<Rtnl, NetError> {
        Ok(Rtnl {
            socket: Socket::open(ROUTE)?,
        })
    }

    /// The link called `name`.
    pub fn link(&mut self, name: &str) -> Result<Link, NetError> {
        let m = Builder::new(RTM_GETLINK, CHANGE)
            .raw(&ifinfo(0, 0, 0))
            .string(IFLA_IFNAME, name);
        match self.socket.request(m) {
            Ok(a) => a
                .first()
                .and_then(|p| parse_link(p))
                .ok_or(NetError::Malformed),
            Err(NetError::Kernel(e)) if e == libc::ENODEV => Err(NetError::NoDevice),
            Err(e) => Err(e),
        }
    }

    /// Sets a link up or down.
    pub fn set_up(&mut self, index: u32, up: bool) -> Result<(), NetError> {
        let flags = if up { IFF_UP } else { 0 };
        self.socket
            .change(Builder::new(RTM_NEWLINK, CHANGE).raw(&ifinfo(index, flags, IFF_UP)))
    }

    /// Sets the MTU of a link.
    pub fn set_mtu(&mut self, index: u32, mtu: u32) -> Result<(), NetError> {
        self.socket.change(
            Builder::new(RTM_NEWLINK, CHANGE)
                .raw(&ifinfo(index, 0, 0))
                .u32(IFLA_MTU, mtu),
        )
    }

    fn addr_msg(kind: u16, flags: u16, index: u32, a: &IpAddr, prefix: u8) -> Builder {
        let mut h = [0u8; 8];
        h[0] = family(a);
        h[1] = prefix;
        h[4..8].copy_from_slice(&index.to_ne_bytes());
        Builder::new(kind, flags)
            .raw(&h)
            .attr(IFA_LOCAL, &bytes(a))
            .attr(IFA_ADDRESS, &bytes(a))
    }

    /// Adds (or refreshes) an address.
    pub fn add_address(&mut self, index: u32, a: IpAddr, prefix: u8) -> Result<(), NetError> {
        self.socket.change(Self::addr_msg(
            RTM_NEWADDR,
            CHANGE | F_CREATE | F_REPLACE,
            index,
            &a,
            prefix,
        ))
    }

    /// Removes every IPv4 address of a link.
    pub fn flush_v4(&mut self, index: u32) -> Result<(), NetError> {
        let mut h = [0u8; 8];
        h[0] = AF_INET;
        let answers = self
            .socket
            .request(Builder::new(RTM_GETADDR, F_REQUEST | F_DUMP).raw(&h))?;
        for p in answers {
            if p.len() < 8 || u32::from_ne_bytes([p[4], p[5], p[6], p[7]]) != index {
                continue;
            }
            if let Some(a) = attr::find(&p[8..], IFA_LOCAL).and_then(ip) {
                let _ = self
                    .socket
                    .change(Self::addr_msg(RTM_DELADDR, CHANGE, index, &a, p[1]));
            }
        }
        Ok(())
    }

    fn route_msg(kind: u16, flags: u16, r: &Route) -> Builder {
        let mut h = [0u8; 12];
        h[0] = family(&r.dst);
        h[1] = r.prefix;
        h[4] = RT_TABLE_MAIN;
        h[5] = r.origin as u8;
        h[6] = if r.gateway.is_some() {
            RT_SCOPE_UNIVERSE
        } else {
            RT_SCOPE_LINK
        };
        h[7] = RTN_UNICAST;
        let mut m = Builder::new(kind, flags).raw(&h);
        if r.prefix > 0 {
            m = m.attr(RTA_DST, &bytes(&r.dst));
        }
        if let Some(g) = &r.gateway {
            m = m.attr(RTA_GATEWAY, &bytes(g));
        }
        m.u32(RTA_OIF, r.oif).u32(RTA_PRIORITY, r.metric)
    }

    /// Adds a route (refused if the same one exists).
    pub fn add_route(&mut self, r: &Route) -> Result<(), NetError> {
        match self
            .socket
            .change(Self::route_msg(RTM_NEWROUTE, CHANGE | F_CREATE | F_EXCL, r))
        {
            Err(NetError::Kernel(e)) if e == libc::EEXIST => Ok(()),
            other => other,
        }
    }

    /// Removes a route.
    pub fn del_route(&mut self, r: &Route) -> Result<(), NetError> {
        self.socket.change(Self::route_msg(RTM_DELROUTE, CHANGE, r))
    }

    /// The routes of the main table of one family (IPv6: `v6`).
    pub fn routes(&mut self, v6: bool) -> Result<Vec<Route>, NetError> {
        let mut h = [0u8; 12];
        h[0] = if v6 { AF_INET6 } else { AF_INET };
        let answers = self
            .socket
            .request(Builder::new(RTM_GETROUTE, F_REQUEST | F_DUMP).raw(&h))?;
        Ok(answers.iter().filter_map(|p| parse_route(p)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_link_and_routes_on_linux() {
        let mut r = Rtnl::open().unwrap();
        let lo = r.link("lo").unwrap();
        assert!(lo.index >= 1);
        assert_eq!(r.link("no-such-if0"), Err(NetError::NoDevice));
        let _ = r.routes(false).unwrap();
        let _ = r.routes(true).unwrap();
    }

    #[test]
    fn parsing_is_bounded() {
        assert_eq!(parse_link(&[0; 15]), None);
        assert_eq!(parse_route(&[0; 11]), None);
        let mut p = vec![
            AF_INET,
            0,
            0,
            0,
            RT_TABLE_MAIN,
            4,
            0,
            RTN_UNICAST,
            0,
            0,
            0,
            0,
        ];
        p.extend_from_slice(&[8, 0, 4, 0, 3, 0, 0, 0]);
        p.extend_from_slice(&[8, 0, 5, 0, 10, 0, 0, 1]);
        let route = parse_route(&p).unwrap();
        assert_eq!(route.oif, 3);
        assert_eq!(route.gateway, Some(IpAddr::from([10, 0, 0, 1])));
        assert_eq!(route.dst, IpAddr::from([0, 0, 0, 0]));
        assert_eq!(ip(&[1, 2, 3]), None);
        p[4] = 255;
        assert_eq!(parse_route(&p), None, "local table");
    }
}
