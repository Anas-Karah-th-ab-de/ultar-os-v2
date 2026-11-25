//! STUN and TURN messages (RFC 8489, RFC 8656) as far as a call needs them: binding (own
//! address as the internet sees it), allocate / refresh / create-permission / channel-bind on the
//! TURN server with long-term credentials, data indications and ChannelData.
//! The TURN server only ever carries already sealed media (see [`crate::e2e::crypto`]).

use hmac::{Hmac, Mac};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Magic cookie of every STUN message.
pub const COOKIE: u32 = 0x2112_A442;

/// Methods.
pub mod method {
    /// Binding.
    pub const BINDING: u16 = 0x001;
    /// Allocate.
    pub const ALLOCATE: u16 = 0x003;
    /// Refresh.
    pub const REFRESH: u16 = 0x004;
    /// Send (indication).
    pub const SEND: u16 = 0x006;
    /// Data (indication).
    pub const DATA: u16 = 0x007;
    /// CreatePermission.
    pub const PERMISSION: u16 = 0x008;
    /// ChannelBind.
    pub const CHANNEL_BIND: u16 = 0x009;
}

/// Classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Request.
    Request,
    /// Indication.
    Indication,
    /// Success response.
    Success,
    /// Error response.
    Error,
}

impl Class {
    const fn bits(self) -> u16 {
        match self {
            Class::Request => 0x000,
            Class::Indication => 0x010,
            Class::Success => 0x100,
            Class::Error => 0x110,
        }
    }
}

/// Attribute types.
pub mod attr {
    /// USERNAME.
    pub const USERNAME: u16 = 0x0006;
    /// MESSAGE-INTEGRITY.
    pub const INTEGRITY: u16 = 0x0008;
    /// ERROR-CODE.
    pub const ERROR: u16 = 0x0009;
    /// CHANNEL-NUMBER.
    pub const CHANNEL: u16 = 0x000C;
    /// LIFETIME.
    pub const LIFETIME: u16 = 0x000D;
    /// XOR-PEER-ADDRESS.
    pub const PEER: u16 = 0x0012;
    /// DATA.
    pub const DATA: u16 = 0x0013;
    /// REALM.
    pub const REALM: u16 = 0x0014;
    /// NONCE.
    pub const NONCE: u16 = 0x0015;
    /// XOR-RELAYED-ADDRESS.
    pub const RELAYED: u16 = 0x0016;
    /// REQUESTED-TRANSPORT.
    pub const TRANSPORT: u16 = 0x0019;
    /// XOR-MAPPED-ADDRESS.
    pub const MAPPED: u16 = 0x0020;
}

/// A STUN message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Msg {
    /// Method.
    pub method: u16,
    /// Class.
    pub class: Class,
    /// Transaction id.
    pub tx: [u8; 12],
    /// Attributes in order.
    pub attrs: Vec<(u16, Vec<u8>)>,
}

/// Long-term credentials of the TURN server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Auth {
    /// User name.
    pub user: String,
    /// Realm (from the server's first answer).
    pub realm: String,
    /// Nonce (from the server's first answer).
    pub nonce: String,
    /// Password.
    pub password: String,
}

impl Auth {
    fn key(&self) -> [u8; 16] {
        md5::compute(format!("{}:{}:{}", self.user, self.realm, self.password)).0
    }
}

fn pad(n: usize) -> usize {
    (n + 3) & !3
}

/// A new random transaction id.
pub fn new_tx() -> [u8; 12] {
    let mut tx = [0u8; 12];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut tx);
    tx
}

impl Msg {
    /// A message without attributes.
    pub fn new(method: u16, class: Class) -> Self {
        Self {
            method,
            class,
            tx: new_tx(),
            attrs: Vec::new(),
        }
    }

    /// Adds an attribute.
    pub fn with(mut self, kind: u16, value: Vec<u8>) -> Self {
        self.attrs.push((kind, value));
        self
    }

    /// The value of the first attribute `kind`.
    pub fn get(&self, kind: u16) -> Option<&[u8]> {
        self.attrs
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, v)| v.as_slice())
    }

    /// Error code of an error response.
    pub fn error_code(&self) -> Option<u16> {
        let v = self.get(attr::ERROR)?;
        (v.len() >= 4).then(|| u16::from(v[2] & 7) * 100 + u16::from(v[3]))
    }

    /// The bytes; with `auth`, USERNAME, REALM, NONCE and MESSAGE-INTEGRITY are added.
    pub fn encode(&self, auth: Option<&Auth>) -> Vec<u8> {
        let mut attrs = self.attrs.clone();
        if let Some(a) = auth {
            attrs.push((attr::USERNAME, a.user.as_bytes().to_vec()));
            attrs.push((attr::REALM, a.realm.as_bytes().to_vec()));
            attrs.push((attr::NONCE, a.nonce.as_bytes().to_vec()));
        }
        let mut body = Vec::new();
        for (k, v) in &attrs {
            body.extend_from_slice(&k.to_be_bytes());
            body.extend_from_slice(&(v.len() as u16).to_be_bytes());
            body.extend_from_slice(v);
            body.resize(pad(body.len()), 0);
        }
        let mi_len = if auth.is_some() { 24 } else { 0 };
        let mut out = Vec::with_capacity(20 + body.len() + mi_len);
        out.extend_from_slice(&(self.method | self.class.bits()).to_be_bytes());
        out.extend_from_slice(&((body.len() + mi_len) as u16).to_be_bytes());
        out.extend_from_slice(&COOKIE.to_be_bytes());
        out.extend_from_slice(&self.tx);
        out.extend_from_slice(&body);
        if let Some(a) = auth {
            let mut mac = <Hmac<sha1::Sha1> as Mac>::new_from_slice(&a.key())
                .unwrap_or_else(|_| unreachable!("HMAC takes any key length"));
            mac.update(&out);
            out.extend_from_slice(&attr::INTEGRITY.to_be_bytes());
            out.extend_from_slice(&20u16.to_be_bytes());
            out.extend_from_slice(&mac.finalize().into_bytes());
        }
        out
    }

    /// Reads a message; `None` for anything that is not STUN.
    pub fn decode(b: &[u8]) -> Option<Msg> {
        if b.len() < 20 || b[0] & 0xC0 != 0 || b[4..8] != COOKIE.to_be_bytes() {
            return None;
        }
        let ty = u16::from_be_bytes([b[0], b[1]]);
        let len = usize::from(u16::from_be_bytes([b[2], b[3]]));
        let body = b.get(20..20 + len)?;
        let class = match ty & 0x110 {
            0x000 => Class::Request,
            0x010 => Class::Indication,
            0x100 => Class::Success,
            _ => Class::Error,
        };
        let mut attrs = Vec::new();
        let mut i = 0;
        while i + 4 <= body.len() {
            let k = u16::from_be_bytes([body[i], body[i + 1]]);
            let n = usize::from(u16::from_be_bytes([body[i + 2], body[i + 3]]));
            let v = body.get(i + 4..i + 4 + n)?;
            attrs.push((k, v.to_vec()));
            i += 4 + pad(n);
        }
        Some(Msg {
            method: ty & !0x110,
            class,
            tx: b[8..20].try_into().ok()?,
            attrs,
        })
    }
}

/// XOR-encodes an IPv4 address (the only family a call uses).
pub fn xor_addr(a: SocketAddr) -> Vec<u8> {
    let port = a.port() ^ (COOKIE >> 16) as u16;
    let ip = match a.ip() {
        IpAddr::V4(v4) => u32::from(v4) ^ COOKIE,
        IpAddr::V6(_) => 0,
    };
    let mut v = vec![0, 1];
    v.extend_from_slice(&port.to_be_bytes());
    v.extend_from_slice(&ip.to_be_bytes());
    v
}

/// Reads an XOR-encoded IPv4 address.
pub fn read_xor_addr(v: &[u8]) -> Option<SocketAddr> {
    if v.len() < 8 || v[1] != 1 {
        return None;
    }
    let port = u16::from_be_bytes([v[2], v[3]]) ^ (COOKIE >> 16) as u16;
    let ip = u32::from_be_bytes([v[4], v[5], v[6], v[7]]) ^ COOKIE;
    Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip)), port))
}

/// A ChannelData frame (RFC 8656 §12.4).
pub fn channel_data(channel: u16, data: &[u8]) -> Vec<u8> {
    let mut v = channel.to_be_bytes().to_vec();
    v.extend_from_slice(&(data.len() as u16).to_be_bytes());
    v.extend_from_slice(data);
    v
}

/// Reads a ChannelData frame: channel and data.
pub fn read_channel_data(b: &[u8]) -> Option<(u16, &[u8])> {
    if b.len() < 4 || !(0x40..=0x7F).contains(&b[0]) {
        return None;
    }
    let ch = u16::from_be_bytes([b[0], b[1]]);
    let n = usize::from(u16::from_be_bytes([b[2], b[3]]));
    Some((ch, b.get(4..4 + n)?))
}

#[cfg(test)]
#[path = "stun_tests.rs"]
mod tests;
