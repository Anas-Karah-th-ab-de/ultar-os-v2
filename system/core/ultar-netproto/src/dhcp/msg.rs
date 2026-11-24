//! DHCP messages: hostile bytes in, checked fields out; the client's
//! messages out.

use crate::Mac;
use std::net::Ipv4Addr;

/// The magic cookie before the options.
pub const COOKIE: [u8; 4] = [99, 130, 83, 99];
/// Fixed part of a message (without options).
pub const FIXED: usize = 240;
/// Longest message accepted.
pub const MAX_LEN: usize = 1500;
/// The broadcast flag (answers come to the broadcast address: the client has
/// no address yet).
pub const BROADCAST: u16 = 0x8000;

/// Option numbers used.
pub mod opt {
    /// Pad.
    pub const PAD: u8 = 0;
    /// Subnet mask.
    pub const MASK: u8 = 1;
    /// Routers.
    pub const ROUTER: u8 = 3;
    /// DNS servers.
    pub const DNS: u8 = 6;
    /// Requested address.
    pub const REQUESTED: u8 = 50;
    /// Lease time.
    pub const LEASE: u8 = 51;
    /// Message type.
    pub const TYPE: u8 = 53;
    /// Server identifier.
    pub const SERVER: u8 = 54;
    /// Parameter request list.
    pub const PARAMS: u8 = 55;
    /// Renewal time (T1).
    pub const T1: u8 = 58;
    /// Rebinding time (T2).
    pub const T2: u8 = 59;
    /// Client identifier.
    pub const CLIENT_ID: u8 = 61;
    /// End.
    pub const END: u8 = 255;
}

/// Why a message was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DhcpError {
    /// Too short, too long or without the cookie.
    Malformed,
    /// An option runs past the end or has a wrong length.
    BadOption,
    /// Not an answer for this client.
    NotForUs,
}

impl std::fmt::Display for DhcpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DhcpError::Malformed => "malformed dhcp message",
            DhcpError::BadOption => "bad dhcp option",
            DhcpError::NotForUs => "dhcp message for another client",
        })
    }
}

impl std::error::Error for DhcpError {}

/// The message types (option 53).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageType {
    /// DHCPDISCOVER.
    Discover = 1,
    /// DHCPOFFER.
    Offer = 2,
    /// DHCPREQUEST.
    Request = 3,
    /// DHCPDECLINE.
    Decline = 4,
    /// DHCPACK.
    Ack = 5,
    /// DHCPNAK.
    Nak = 6,
    /// DHCPRELEASE.
    Release = 7,
}

impl MessageType {
    fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            1 => Self::Discover,
            2 => Self::Offer,
            3 => Self::Request,
            4 => Self::Decline,
            5 => Self::Ack,
            6 => Self::Nak,
            7 => Self::Release,
            _ => return None,
        })
    }
}

/// A DHCP message (the fields this client uses).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// 1 request, 2 reply.
    pub op: u8,
    /// Transaction id.
    pub xid: u32,
    /// Flags.
    pub flags: u16,
    /// Client address (renewing).
    pub ciaddr: Ipv4Addr,
    /// "Your" address (offered).
    pub yiaddr: Ipv4Addr,
    /// Client hardware address.
    pub chaddr: Mac,
    /// Options in order (number, value); pad and end left out.
    pub options: Vec<(u8, Vec<u8>)>,
}

fn addr(b: &[u8]) -> Ipv4Addr {
    Ipv4Addr::new(b[0], b[1], b[2], b[3])
}

impl Message {
    /// Reads a message.
    pub fn parse(b: &[u8]) -> Result<Message, DhcpError> {
        if b.len() < FIXED || b.len() > MAX_LEN || b[236..240] != COOKIE {
            return Err(DhcpError::Malformed);
        }
        let mut chaddr = [0u8; 6];
        chaddr.copy_from_slice(&b[28..34]);
        if b[1] != 1 || b[2] != 6 {
            return Err(DhcpError::Malformed);
        }
        Ok(Message {
            op: b[0],
            xid: u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
            flags: u16::from_be_bytes([b[10], b[11]]),
            ciaddr: addr(&b[12..16]),
            yiaddr: addr(&b[16..20]),
            chaddr,
            options: parse_options(&b[FIXED..])?,
        })
    }

    /// The bytes of the message (padded to the 300 bytes of BOOTP).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = vec![0u8; FIXED];
        b[0] = self.op;
        b[1] = 1;
        b[2] = 6;
        b[4..8].copy_from_slice(&self.xid.to_be_bytes());
        b[10..12].copy_from_slice(&self.flags.to_be_bytes());
        b[12..16].copy_from_slice(&self.ciaddr.octets());
        b[16..20].copy_from_slice(&self.yiaddr.octets());
        b[28..34].copy_from_slice(&self.chaddr);
        b[236..240].copy_from_slice(&COOKIE);
        for (code, value) in &self.options {
            b.push(*code);
            b.push(value.len().min(255) as u8);
            b.extend_from_slice(&value[..value.len().min(255)]);
        }
        b.push(opt::END);
        if b.len() < 300 {
            b.resize(300, 0);
        }
        b
    }

    /// The value of option `code`.
    pub fn option(&self, code: u8) -> Option<&[u8]> {
        self.options
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, v)| v.as_slice())
    }

    /// The message type.
    pub fn kind(&self) -> Option<MessageType> {
        match self.option(opt::TYPE)? {
            [t] => MessageType::from_byte(*t),
            _ => None,
        }
    }

    /// An option holding one address.
    pub fn address(&self, code: u8) -> Option<Ipv4Addr> {
        self.option(code).filter(|v| v.len() == 4).map(addr)
    }

    /// An option holding a list of addresses.
    pub fn addresses(&self, code: u8) -> Vec<Ipv4Addr> {
        match self.option(code) {
            Some(v) if v.len().is_multiple_of(4) => v.chunks_exact(4).map(addr).collect(),
            _ => Vec::new(),
        }
    }

    /// An option holding a 32-bit number of seconds.
    pub fn seconds(&self, code: u8) -> Option<u32> {
        self.option(code)
            .filter(|v| v.len() == 4)
            .map(|v| u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
    }
}

fn parse_options(mut b: &[u8]) -> Result<Vec<(u8, Vec<u8>)>, DhcpError> {
    let mut out = Vec::new();
    loop {
        match b {
            [] | [opt::END, ..] => return Ok(out),
            [opt::PAD, rest @ ..] => b = rest,
            [code, len, rest @ ..] if rest.len() >= usize::from(*len) => {
                let (value, tail) = rest.split_at(usize::from(*len));
                if out.len() >= 64 {
                    return Err(DhcpError::BadOption);
                }
                out.push((*code, value.to_vec()));
                b = tail;
            }
            _ => return Err(DhcpError::BadOption),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_accessors() {
        let m = Message {
            op: 2,
            xid: 0xdead_beef,
            flags: BROADCAST,
            ciaddr: Ipv4Addr::UNSPECIFIED,
            yiaddr: Ipv4Addr::new(10, 0, 0, 7),
            chaddr: [2, 0, 0, 0, 0, 9],
            options: vec![
                (opt::TYPE, vec![2]),
                (opt::SERVER, vec![10, 0, 0, 1]),
                (opt::DNS, vec![1, 1, 1, 1, 9, 9, 9, 9]),
                (opt::LEASE, 3600u32.to_be_bytes().to_vec()),
            ],
        };
        let bytes = m.to_bytes();
        assert_eq!(bytes.len(), 300);
        let back = Message::parse(&bytes).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.kind(), Some(MessageType::Offer));
        assert_eq!(back.address(opt::SERVER), Some(Ipv4Addr::new(10, 0, 0, 1)));
        assert_eq!(back.addresses(opt::DNS).len(), 2);
        assert_eq!(back.seconds(opt::LEASE), Some(3600));
        assert_eq!(back.seconds(opt::T1), None);
        assert_eq!(back.address(opt::DNS), None);
    }

    #[test]
    fn hostile_messages() {
        let good = Message {
            op: 2,
            xid: 1,
            flags: 0,
            ciaddr: Ipv4Addr::UNSPECIFIED,
            yiaddr: Ipv4Addr::UNSPECIFIED,
            chaddr: [0; 6],
            options: vec![(opt::TYPE, vec![5])],
        }
        .to_bytes();
        assert_eq!(Message::parse(&good[..239]), Err(DhcpError::Malformed));
        let mut t = good.clone();
        t[236] = 0;
        assert_eq!(Message::parse(&t), Err(DhcpError::Malformed));
        let mut t = good.clone();
        t[2] = 8;
        assert_eq!(Message::parse(&t), Err(DhcpError::Malformed));
        let mut t = good[..FIXED].to_vec();
        t.extend_from_slice(&[53, 5, 1]);
        assert_eq!(Message::parse(&t), Err(DhcpError::BadOption));
        let mut t = good[..FIXED].to_vec();
        for _ in 0..70 {
            t.extend_from_slice(&[12, 0]);
        }
        assert_eq!(Message::parse(&t), Err(DhcpError::BadOption));
        assert_eq!(Message::parse(&vec![0; 1501]), Err(DhcpError::Malformed));
        let mut t = good.clone();
        t[242] = 9;
        assert_eq!(Message::parse(&t).unwrap().kind(), None);
        for e in [
            DhcpError::Malformed,
            DhcpError::BadOption,
            DhcpError::NotForUs,
        ] {
            assert!(!e.to_string().is_empty());
        }
        let mut seed = 77u32;
        for _ in 0..5000 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let mut t = good.clone();
            let at = FIXED + seed as usize % (t.len() - FIXED);
            t[at] = (seed >> 9) as u8;
            t.truncate(at + 1 + (seed as usize >> 24) % 8);
            let _ = Message::parse(&t);
        }
    }
}
