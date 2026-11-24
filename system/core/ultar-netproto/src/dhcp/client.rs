//! The lease state machine of the client (RFC 2131 4.4): selecting,
//! requesting, bound, renewing, rebinding. Time is given in milliseconds by
//! the caller; retransmissions back off 4, 8, 16, 32, 64 s.

use super::msg::{opt, Message, MessageType, BROADCAST};
use crate::Mac;
use std::net::Ipv4Addr;

/// Shortest lease taken (seconds): shorter ones are stretched.
pub const MIN_LEASE: u32 = 60;
/// Longest wait between two sends.
const MAX_BACKOFF_MS: u64 = 64_000;
/// Requests sent before starting over with a discover.
const MAX_REQUESTS: u32 = 4;
/// Most DNS servers taken.
pub const MAX_DNS: usize = 3;

/// An address lease.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lease {
    /// The address.
    pub address: Ipv4Addr,
    /// Prefix length of the subnet.
    pub prefix: u8,
    /// Default router.
    pub router: Option<Ipv4Addr>,
    /// DNS servers (≤ 3).
    pub dns: Vec<Ipv4Addr>,
    /// The server that gave it.
    pub server: Ipv4Addr,
    /// Lease time (seconds).
    pub seconds: u32,
}

/// What the caller does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Send to the broadcast address, port 67.
    Broadcast(Vec<u8>),
    /// Send to this server, port 67.
    Unicast(Ipv4Addr, Vec<u8>),
    /// Configure the interface with the lease (new or changed).
    Configure(Lease),
    /// Take the address away (lease lost).
    Deconfigure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    Selecting,
    Requesting { offer: Lease, sent: u32 },
    Bound,
    Renewing,
    Rebinding,
}

/// One DHCP client of one interface.
#[derive(Debug)]
pub struct Client {
    mac: Mac,
    xid: u32,
    state: State,
    lease: Option<Lease>,
    bound_at: u64,
    next_at: u64,
    backoff: u64,
}

fn prefix_of(mask: Ipv4Addr) -> Option<u8> {
    let bits = u32::from(mask);
    let ones = bits.leading_ones();
    (bits.checked_shl(ones).unwrap_or(0) == 0).then_some(ones as u8)
}

fn classful(a: Ipv4Addr) -> u8 {
    match a.octets()[0] {
        0..=127 => 8,
        128..=191 => 16,
        _ => 24,
    }
}

fn usable(a: Ipv4Addr) -> bool {
    !(a.is_unspecified() || a.is_broadcast() || a.is_multicast() || a.is_loopback())
}

impl Client {
    /// A client for the interface with address `mac`; `xid` is a random
    /// transaction id; the first discover goes out at `now`.
    pub fn new(mac: Mac, xid: u32, now: u64) -> Client {
        Client {
            mac,
            xid,
            state: State::Selecting,
            lease: None,
            bound_at: now,
            next_at: now,
            backoff: 4_000,
        }
    }

    /// The lease in use.
    pub fn lease(&self) -> Option<&Lease> {
        self.lease.as_ref()
    }

    /// When [`Client::poll`] must run next.
    pub fn next_deadline(&self) -> u64 {
        self.next_at
    }

    fn message(&self, kind: MessageType, ciaddr: Ipv4Addr, extra: Vec<(u8, Vec<u8>)>) -> Vec<u8> {
        let mut options = vec![(opt::TYPE, vec![kind as u8])];
        let mut id = vec![1];
        id.extend_from_slice(&self.mac);
        options.push((opt::CLIENT_ID, id));
        options.extend(extra);
        options.push((
            opt::PARAMS,
            vec![
                opt::MASK,
                opt::ROUTER,
                opt::DNS,
                opt::LEASE,
                opt::T1,
                opt::T2,
            ],
        ));
        Message {
            op: 1,
            xid: self.xid,
            flags: if ciaddr.is_unspecified() {
                BROADCAST
            } else {
                0
            },
            ciaddr,
            yiaddr: Ipv4Addr::UNSPECIFIED,
            chaddr: self.mac,
            options,
        }
        .to_bytes()
    }

    fn wait(&mut self, now: u64) {
        self.next_at = now + self.backoff;
        self.backoff = (self.backoff * 2).min(MAX_BACKOFF_MS);
    }

    fn restart(&mut self, now: u64) -> Vec<Action> {
        self.state = State::Selecting;
        self.xid = self.xid.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.backoff = 4_000;
        self.next_at = now;
        match self.lease.take() {
            Some(_) => vec![Action::Deconfigure],
            None => Vec::new(),
        }
    }

    fn ms(&self, part: u64) -> u64 {
        let secs = u64::from(self.lease.as_ref().map_or(MIN_LEASE, |l| l.seconds));
        self.bound_at + secs * part / 8 * 1000
    }

    /// Timers: retransmissions, renewal (T1, 1/2), rebinding (T2, 7/8),
    /// expiry.
    pub fn poll(&mut self, now: u64) -> Vec<Action> {
        if now < self.next_at {
            return Vec::new();
        }
        match self.state.clone() {
            State::Selecting => {
                self.wait(now);
                vec![Action::Broadcast(self.message(
                    MessageType::Discover,
                    Ipv4Addr::UNSPECIFIED,
                    vec![],
                ))]
            }
            State::Requesting { offer, sent } if sent < MAX_REQUESTS => {
                self.state = State::Requesting {
                    offer: offer.clone(),
                    sent: sent + 1,
                };
                self.wait(now);
                vec![Action::Broadcast(self.request(&offer))]
            }
            State::Requesting { .. } => self.restart(now),
            State::Bound | State::Renewing | State::Rebinding if now >= self.ms(8) => {
                self.restart(now)
            }
            State::Bound | State::Renewing if now < self.ms(7) => {
                self.state = State::Renewing;
                self.next_at = (now + 60_000).min(self.ms(7));
                let Some(lease) = &self.lease else {
                    return self.restart(now);
                };
                vec![Action::Unicast(
                    lease.server,
                    self.message(MessageType::Request, lease.address, vec![]),
                )]
            }
            State::Bound | State::Renewing | State::Rebinding => {
                self.state = State::Rebinding;
                self.next_at = (now + 60_000).min(self.ms(8));
                let Some(lease) = &self.lease else {
                    return self.restart(now);
                };
                vec![Action::Broadcast(self.message(
                    MessageType::Request,
                    lease.address,
                    vec![],
                ))]
            }
        }
    }

    fn request(&self, offer: &Lease) -> Vec<u8> {
        self.message(
            MessageType::Request,
            Ipv4Addr::UNSPECIFIED,
            vec![
                (opt::REQUESTED, offer.address.octets().to_vec()),
                (opt::SERVER, offer.server.octets().to_vec()),
            ],
        )
    }

    fn lease_of(&self, m: &Message) -> Option<Lease> {
        let server = m.address(opt::SERVER)?;
        let address = m.yiaddr;
        if !usable(address) {
            return None;
        }
        let prefix = match m.address(opt::MASK) {
            Some(mask) => prefix_of(mask).filter(|p| (1..=30).contains(p))?,
            None => classful(address),
        };
        let mut dns: Vec<Ipv4Addr> = m
            .addresses(opt::DNS)
            .into_iter()
            .filter(|a| usable(*a))
            .collect();
        dns.truncate(MAX_DNS);
        Some(Lease {
            address,
            prefix,
            router: m.addresses(opt::ROUTER).into_iter().find(|a| usable(*a)),
            dns,
            server,
            seconds: m.seconds(opt::LEASE).unwrap_or(MIN_LEASE).max(MIN_LEASE),
        })
    }

    /// One datagram from port 67.
    pub fn receive(&mut self, bytes: &[u8], now: u64) -> Vec<Action> {
        let Ok(m) = Message::parse(bytes) else {
            return Vec::new();
        };
        if m.op != 2 || m.xid != self.xid || m.chaddr != self.mac {
            return Vec::new();
        }
        match (self.state.clone(), m.kind()) {
            (State::Selecting, Some(MessageType::Offer)) => {
                let Some(offer) = self.lease_of(&m) else {
                    return Vec::new();
                };
                self.backoff = 4_000;
                self.state = State::Requesting { offer, sent: 0 };
                self.next_at = now;
                self.poll(now)
            }
            (
                State::Requesting { .. } | State::Renewing | State::Rebinding,
                Some(MessageType::Ack),
            ) => self.bound(&m, now),
            (
                State::Requesting { .. } | State::Renewing | State::Rebinding,
                Some(MessageType::Nak),
            ) => self.restart(now),
            _ => Vec::new(),
        }
    }

    fn bound(&mut self, m: &Message, now: u64) -> Vec<Action> {
        let Some(lease) = self.lease_of(m) else {
            return Vec::new();
        };
        if let State::Requesting { offer, .. } = &self.state {
            if offer.address != lease.address {
                return Vec::new();
            }
        }
        let changed = self.lease.as_ref() != Some(&lease);
        self.state = State::Bound;
        self.bound_at = now;
        self.lease = Some(lease.clone());
        self.backoff = 4_000;
        self.next_at = self.ms(4);
        if changed {
            vec![Action::Configure(lease)]
        } else {
            Vec::new()
        }
    }

    /// A release for the server when the interface is given up (send it
    /// to the server, then deconfigure).
    pub fn release(&self) -> Option<(Ipv4Addr, Vec<u8>)> {
        let lease = self.lease.as_ref()?;
        let extra = vec![(opt::SERVER, lease.server.octets().to_vec())];
        Some((
            lease.server,
            self.message(MessageType::Release, lease.address, extra),
        ))
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
