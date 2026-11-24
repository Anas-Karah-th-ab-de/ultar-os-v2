//! WireGuard `.conf` files as `wg-quick` writes them, read strictly: one
//! `[Interface]`, exactly one `[Peer]`, only the keys below; commands
//! (`PreUp`, `PostUp`, …) and routing tricks (`Table`, `FwMark`) are refused,
//! never run. Every refusal names its line. Keys are wiped when dropped.
//!
//! `[Interface]`: `PrivateKey` (required), `Address` (required), `DNS`, `MTU`,
//! `ListenPort`. `[Peer]`: `PublicKey` (required), `PresharedKey`,
//! `AllowedIPs` (required), `Endpoint` (required), `PersistentKeepalive`.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use ultar_bus::messages::vpn::ConfProblem;
use zeroize::Zeroize;

/// Longest file.
pub const MAX_FILE: usize = 16 * 1024;
/// Longest line.
pub const MAX_LINE: usize = 1024;
/// Most addresses or networks in one list.
pub const MAX_LIST: usize = 64;

/// A 32-byte key, wiped when dropped, never printed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key(pub [u8; 32]);

impl Drop for Key {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(..)")
    }
}

/// A network: address and prefix length.
pub type Net = (IpAddr, u8);

/// The server: host (address or name) and port.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    /// Address or DNS name.
    pub host: String,
    /// UDP port.
    pub port: u16,
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.host.contains(':') {
            write!(f, "[{}]:{}", self.host, self.port)
        } else {
            write!(f, "{}:{}", self.host, self.port)
        }
    }
}

/// A checked tunnel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TunnelConf {
    /// Own private key.
    pub private_key: Key,
    /// Own addresses.
    pub addresses: Vec<Net>,
    /// DNS servers.
    pub dns: Vec<IpAddr>,
    /// MTU.
    pub mtu: Option<u16>,
    /// UDP port to listen on.
    pub listen_port: Option<u16>,
    /// The server's public key.
    pub peer_key: [u8; 32],
    /// Pre-shared key.
    pub preshared: Option<Key>,
    /// Networks routed through the tunnel.
    pub allowed: Vec<Net>,
    /// The server.
    pub endpoint: Endpoint,
    /// Keep-alive interval (s).
    pub keepalive: Option<u16>,
}

impl TunnelConf {
    /// Everything goes through the tunnel (a default route is allowed).
    pub fn full_tunnel(&self) -> bool {
        self.allowed.iter().any(|(_, p)| *p == 0)
    }
}

/// A refusal at a line.
pub type Refusal = (ConfProblem, u32);

fn key(v: &str) -> Result<Key, ConfProblem> {
    let mut bytes = base64::engine::general_purpose::STANDARD
        .decode(v)
        .map_err(|_| ConfProblem::BadKey)?;
    let out: Result<[u8; 32], _> = bytes.as_slice().try_into();
    bytes.zeroize();
    out.map(Key).map_err(|_| ConfProblem::BadKey)
}

fn net(v: &str) -> Result<Net, ConfProblem> {
    let (a, p) = match v.split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (v, None),
    };
    let addr: IpAddr = a.trim().parse().map_err(|_| ConfProblem::BadAddress)?;
    let max = if addr.is_ipv4() { 32 } else { 128 };
    let prefix = match p {
        Some(p) => p
            .trim()
            .parse::<u8>()
            .map_err(|_| ConfProblem::BadAddress)?,
        None => max,
    };
    if prefix > max {
        return Err(ConfProblem::BadAddress);
    }
    Ok((addr, prefix))
}

fn list<T>(v: &str, one: fn(&str) -> Result<T, ConfProblem>) -> Result<Vec<T>, ConfProblem> {
    let items: Vec<&str> = v
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if items.is_empty() || items.len() > MAX_LIST {
        return Err(ConfProblem::BadAddress);
    }
    items.into_iter().map(one).collect()
}

fn host_ok(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        && !h.starts_with('-')
}

/// Reads `host:port` or `[v6]:port`.
pub fn endpoint(v: &str) -> Result<Endpoint, ConfProblem> {
    let (host, port) = if let Some(rest) = v.strip_prefix('[') {
        let (h, p) = rest.split_once("]:").ok_or(ConfProblem::BadEndpoint)?;
        h.parse::<std::net::Ipv6Addr>()
            .map_err(|_| ConfProblem::BadEndpoint)?;
        (h, p)
    } else {
        let (h, p) = v.rsplit_once(':').ok_or(ConfProblem::BadEndpoint)?;
        if h.parse::<std::net::Ipv4Addr>().is_err() && !host_ok(h) {
            return Err(ConfProblem::BadEndpoint);
        }
        (h, p)
    };
    let port: u16 = port.parse().map_err(|_| ConfProblem::BadEndpoint)?;
    if port == 0 {
        return Err(ConfProblem::BadEndpoint);
    }
    Ok(Endpoint {
        host: host.to_owned(),
        port,
    })
}

fn number(v: &str, lo: u16, hi: u16) -> Result<u16, ConfProblem> {
    v.parse::<u16>()
        .ok()
        .filter(|n| (lo..=hi).contains(n))
        .ok_or(ConfProblem::BadNumber)
}

#[derive(Default)]
struct Parts {
    private_key: Option<Key>,
    addresses: Option<Vec<Net>>,
    dns: Option<Vec<IpAddr>>,
    mtu: Option<u16>,
    listen_port: Option<u16>,
    peer_key: Option<Key>,
    preshared: Option<Key>,
    allowed: Option<Vec<Net>>,
    endpoint: Option<Endpoint>,
    keepalive: Option<u16>,
}

fn set<T>(slot: &mut Option<T>, v: T) -> Result<(), ConfProblem> {
    if slot.is_some() {
        return Err(ConfProblem::Duplicate);
    }
    *slot = Some(v);
    Ok(())
}

fn ip(v: &str) -> Result<IpAddr, ConfProblem> {
    v.parse().map_err(|_| ConfProblem::BadAddress)
}

impl Parts {
    fn take(&mut self, peer: bool, k: &str, v: &str) -> Result<(), ConfProblem> {
        match (peer, k.to_ascii_lowercase().as_str()) {
            (false, "privatekey") => set(&mut self.private_key, key(v)?),
            (false, "address") => set(&mut self.addresses, list(v, net)?),
            (false, "dns") => set(&mut self.dns, list(v, ip)?),
            (false, "mtu") => set(&mut self.mtu, number(v, 1280, 9000)?),
            (false, "listenport") => set(&mut self.listen_port, number(v, 1, 65535)?),
            (true, "publickey") => set(&mut self.peer_key, key(v)?),
            (true, "presharedkey") => set(&mut self.preshared, key(v)?),
            (true, "allowedips") => set(&mut self.allowed, list(v, net)?),
            (true, "endpoint") => set(&mut self.endpoint, endpoint(v)?),
            (true, "persistentkeepalive") => set(&mut self.keepalive, number(v, 0, 65535)?),
            _ => Err(ConfProblem::UnknownKey),
        }
    }

    fn finish(self) -> Result<TunnelConf, ConfProblem> {
        let missing = ConfProblem::Missing;
        let mut dns = self.dns.unwrap_or_default();
        dns.truncate(4);
        Ok(TunnelConf {
            private_key: self.private_key.ok_or(missing)?,
            addresses: self.addresses.ok_or(missing)?,
            dns,
            mtu: self.mtu,
            listen_port: self.listen_port,
            peer_key: self.peer_key.ok_or(missing)?.0,
            preshared: self.preshared,
            allowed: self.allowed.ok_or(missing)?,
            endpoint: self.endpoint.ok_or(missing)?,
            keepalive: self.keepalive.filter(|k| *k > 0),
        })
    }
}

/// Reads a `.conf` file.
pub fn parse(text: &str) -> Result<TunnelConf, Refusal> {
    if text.len() > MAX_FILE || text.contains('\0') {
        return Err((ConfProblem::Unreadable, 0));
    }
    let mut parts = Parts::default();
    let (mut section, mut peers) = (None::<bool>, 0u32);
    for (i, raw) in text.lines().enumerate() {
        let n = i as u32 + 1;
        if raw.len() > MAX_LINE {
            return Err((ConfProblem::Unreadable, n));
        }
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            section = match line.to_ascii_lowercase().as_str() {
                "[interface]" if section.is_none() => Some(false),
                "[peer]" => {
                    peers += 1;
                    if peers > 1 {
                        return Err((ConfProblem::PeerCount, n));
                    }
                    Some(true)
                }
                _ => return Err((ConfProblem::Syntax, n)),
            };
            continue;
        }
        let (Some(peer), Some((k, v))) = (section, line.split_once('=')) else {
            return Err((ConfProblem::Syntax, n));
        };
        parts.take(peer, k.trim(), v.trim()).map_err(|p| (p, n))?;
    }
    if peers != 1 {
        return Err((ConfProblem::PeerCount, 0));
    }
    parts.finish().map_err(|p| (p, 0))
}

#[cfg(test)]
#[path = "conf_tests.rs"]
mod tests;
