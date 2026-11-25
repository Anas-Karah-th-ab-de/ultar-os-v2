//! The call's UDP socket: own addresses (host, as seen from the internet, relayed by the TURN
//! server), sending on a [`Route`] and receiving with the route a packet came on.
//!
//! Everything sent here is already sealed; the TURN server sees only ciphertext and two
//! addresses, under a user name that is random for every call (`/v1/turn-cred`).

use crate::e2e::stun::{self, attr, method, Auth, Class, Msg};
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

/// How long one STUN/TURN request waits for its answer.
const ASK_WAIT: Duration = Duration::from_millis(700);
/// Tries of one request.
const ASK_TRIES: u32 = 3;
/// Lifetime asked for the allocation (seconds).
const LIFETIME: u32 = 600;
/// Refresh allocation, permissions and channels this often.
const KEEP: Duration = Duration::from_secs(120);
/// Largest datagram.
pub const MAX_DATAGRAM: usize = 1500;

/// How a packet travels between the two devices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Route {
    /// Straight to (or from) this address.
    Direct(SocketAddr),
    /// Through this device's TURN allocation to (or from) this address.
    Relayed(SocketAddr),
}

impl Route {
    /// Lower is better: a direct route beats one through the relay.
    pub fn rank(self) -> u8 {
        match self {
            Route::Direct(_) => 0,
            Route::Relayed(_) => 1,
        }
    }
}

/// Where and how to reach the TURN server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnConfig {
    /// Its address (UDP).
    pub server: SocketAddr,
    /// User name (`<expiry>:<random>`).
    pub user: String,
    /// Password.
    pub password: String,
}

struct Turn {
    server: SocketAddr,
    auth: Auth,
    relayed: SocketAddr,
    channels: HashMap<SocketAddr, u16>,
    kept: Instant,
}

/// The socket of one call.
pub struct Net {
    /// Only the relay: no own address is told, nothing is sent around it.
    pub relay_only: bool,
    sock: UdpSocket,
    mapped: Option<SocketAddr>,
    turn: Option<Turn>,
    stun_server: Option<SocketAddr>,
}

/// The local address a packet to `to` would leave from (no packet is sent).
fn local_ip(to: SocketAddr) -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect(to).ok()?;
    s.local_addr().ok().map(|a| a.ip())
}

impl Net {
    /// Opens the socket and learns its addresses. Without `turn` only host addresses exist
    /// (calls in one network); a TURN server that does not answer leaves the host addresses.
    pub fn open(turn: Option<&TurnConfig>) -> io::Result<Net> {
        let sock = UdpSocket::bind("0.0.0.0:0")?;
        let mut net = Net {
            relay_only: false,
            sock,
            mapped: None,
            turn: None,
            stun_server: turn.map(|t| t.server),
        };
        if let Some(t) = turn {
            net.mapped = net.binding(t.server);
            net.turn = net.allocate(t);
        }
        Ok(net)
    }

    /// This device's candidate addresses for the other side.
    pub fn candidates(&self) -> Vec<SocketAddr> {
        if self.relay_only {
            return self.turn.as_ref().map(|t| t.relayed).into_iter().collect();
        }
        let port = self.sock.local_addr().map(|a| a.port()).unwrap_or(0);
        let probe = self
            .stun_server
            .unwrap_or_else(|| SocketAddr::from(([192, 0, 2, 1], 9)));
        let mut out = Vec::new();
        if let Some(ip) = local_ip(probe) {
            out.push(SocketAddr::new(ip, port));
        }
        out.extend(self.mapped);
        out.extend(self.turn.as_ref().map(|t| t.relayed));
        out.dedup();
        out
    }

    /// True when packets can go through the TURN server.
    pub fn has_relay(&self) -> bool {
        self.turn.is_some()
    }

    fn ask(&self, to: SocketAddr, m: &Msg, auth: Option<&Auth>) -> Option<Msg> {
        let bytes = m.encode(auth);
        let mut buf = [0u8; MAX_DATAGRAM];
        self.sock.set_read_timeout(Some(ASK_WAIT)).ok()?;
        for _ in 0..ASK_TRIES {
            self.sock.send_to(&bytes, to).ok()?;
            let deadline = Instant::now() + ASK_WAIT;
            while Instant::now() < deadline {
                let Ok((n, from)) = self.sock.recv_from(&mut buf) else {
                    break;
                };
                match Msg::decode(&buf[..n]) {
                    Some(r) if from == to && r.tx == m.tx => return Some(r),
                    _ => {}
                }
            }
        }
        None
    }

    fn binding(&self, server: SocketAddr) -> Option<SocketAddr> {
        let r = self.ask(server, &Msg::new(method::BINDING, Class::Request), None)?;
        stun::read_xor_addr(r.get(attr::MAPPED)?)
    }

    fn allocate(&self, t: &TurnConfig) -> Option<Turn> {
        let req = || {
            Msg::new(method::ALLOCATE, Class::Request)
                .with(attr::TRANSPORT, vec![17, 0, 0, 0])
                .with(attr::LIFETIME, LIFETIME.to_be_bytes().to_vec())
        };
        let first = self.ask(t.server, &req(), None)?;
        let text = |k| String::from_utf8(first.get(k)?.to_vec()).ok();
        let auth = Auth {
            user: t.user.clone(),
            realm: text(attr::REALM)?,
            nonce: text(attr::NONCE)?,
            password: t.password.clone(),
        };
        let ok = self.ask(t.server, &req(), Some(&auth))?;
        if ok.class != Class::Success {
            return None;
        }
        Some(Turn {
            server: t.server,
            relayed: stun::read_xor_addr(ok.get(attr::RELAYED)?)?,
            auth,
            channels: HashMap::new(),
            kept: Instant::now(),
        })
    }

    /// Lets packets of these peers through the relay (permission and a channel each).
    pub fn permit(&mut self, peers: &[SocketAddr]) {
        let Some(turn) = self.turn.as_ref() else {
            return;
        };
        let (server, auth) = (turn.server, turn.auth.clone());
        let mut next = 0x4000 + turn.channels.len() as u16;
        for peer in peers {
            if self
                .turn
                .as_ref()
                .is_some_and(|t| t.channels.contains_key(peer))
            {
                continue;
            }
            let bind = Msg::new(method::CHANNEL_BIND, Class::Request)
                .with(attr::CHANNEL, [next.to_be_bytes(), [0, 0]].concat())
                .with(attr::PEER, stun::xor_addr(*peer));
            let ok = self
                .ask(server, &bind, Some(&auth))
                .is_some_and(|r| r.class == Class::Success);
            if let (true, Some(t)) = (ok, self.turn.as_mut()) {
                t.channels.insert(*peer, next);
                next += 1;
            }
        }
    }

    /// Keeps the allocation and its channels alive (call this every second or so).
    pub fn keep(&mut self) {
        let due = self.turn.as_ref().is_some_and(|t| t.kept.elapsed() >= KEEP);
        if !due {
            return;
        }
        let Some(t) = self.turn.as_ref() else { return };
        let (server, auth) = (t.server, t.auth.clone());
        let peers: Vec<(SocketAddr, u16)> = t.channels.iter().map(|(p, c)| (*p, *c)).collect();
        let refresh = Msg::new(method::REFRESH, Class::Request)
            .with(attr::LIFETIME, LIFETIME.to_be_bytes().to_vec());
        let _ = self.ask(server, &refresh, Some(&auth));
        for (peer, ch) in peers {
            let bind = Msg::new(method::CHANNEL_BIND, Class::Request)
                .with(attr::CHANNEL, [ch.to_be_bytes(), [0, 0]].concat())
                .with(attr::PEER, stun::xor_addr(peer));
            let _ = self.ask(server, &bind, Some(&auth));
        }
        if let Some(t) = self.turn.as_mut() {
            t.kept = Instant::now();
        }
    }

    /// Gives the allocation back (end of the call).
    pub fn close(&mut self) {
        if let Some(t) = self.turn.take() {
            let gone = Msg::new(method::REFRESH, Class::Request)
                .with(attr::LIFETIME, 0u32.to_be_bytes().to_vec());
            let _ = self.ask(t.server, &gone, Some(&t.auth));
        }
    }

    /// Sends `data` on `route`.
    pub fn send(&self, route: Route, data: &[u8]) -> io::Result<()> {
        match route {
            Route::Direct(_) if self.relay_only => Err(io::ErrorKind::PermissionDenied.into()),
            Route::Direct(to) => self.sock.send_to(data, to).map(|_| ()),
            Route::Relayed(peer) => {
                let t = self.turn.as_ref().ok_or(io::ErrorKind::NotConnected)?;
                let ch = t.channels.get(&peer).ok_or(io::ErrorKind::NotConnected)?;
                self.sock
                    .send_to(&stun::channel_data(*ch, data), t.server)
                    .map(|_| ())
            }
        }
    }

    /// Waits up to `wait` for a packet of the other side: its route and bytes.
    pub fn recv(&self, wait: Duration) -> Option<(Route, Vec<u8>)> {
        let mut buf = [0u8; MAX_DATAGRAM];
        self.sock
            .set_read_timeout(Some(wait.max(Duration::from_millis(1))))
            .ok()?;
        let deadline = Instant::now() + wait;
        loop {
            let (n, from) = self.sock.recv_from(&mut buf).ok()?;
            if let Some(got) = self.classify(&buf[..n], from) {
                return Some(got);
            }
            if Instant::now() >= deadline {
                return None;
            }
        }
    }

    fn classify(&self, b: &[u8], from: SocketAddr) -> Option<(Route, Vec<u8>)> {
        let Some(t) = self.turn.as_ref().filter(|t| t.server == from) else {
            return Some((Route::Direct(from), b.to_vec()));
        };
        if let Some((ch, data)) = stun::read_channel_data(b) {
            let peer = t.channels.iter().find(|(_, c)| **c == ch)?.0;
            return Some((Route::Relayed(*peer), data.to_vec()));
        }
        let m = Msg::decode(b)?;
        if m.method == method::DATA && m.class == Class::Indication {
            let peer = stun::read_xor_addr(m.get(attr::PEER)?)?;
            return Some((Route::Relayed(peer), m.get(attr::DATA)?.to_vec()));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_sockets_in_one_network_reach_each_other_directly() {
        let (a, b) = (Net::open(None).unwrap(), Net::open(None).unwrap());
        assert!(!a.has_relay());
        let port = b.sock.local_addr().unwrap().port();
        let to = SocketAddr::from(([127, 0, 0, 1], port));
        a.send(Route::Direct(to), b"sealed").unwrap();
        let (route, data) = b.recv(Duration::from_secs(2)).unwrap();
        assert_eq!(data, b"sealed");
        assert!(matches!(route, Route::Direct(_)));
        assert!(a.send(Route::Relayed(to), b"x").is_err(), "no relay");
        assert!(Route::Direct(to).rank() < Route::Relayed(to).rank());
        assert!(b.recv(Duration::from_millis(50)).is_none());
    }
}
