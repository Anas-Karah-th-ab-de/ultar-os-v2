//! Netlink messages (`nlmsghdr`) and attributes (`nlattr`): a builder for
//! requests and bounded readers for whatever comes back.

use crate::NetError;

/// Size of `nlmsghdr`.
pub const HEADER: usize = 16;
/// Message type of errors and acknowledgements.
pub const NLMSG_ERROR: u16 = 2;
/// Message type ending a dump.
pub const NLMSG_DONE: u16 = 3;
/// Request flag.
pub const F_REQUEST: u16 = 1;
/// Part of a multi-part answer.
pub const F_MULTI: u16 = 2;
/// Ask for an acknowledgement.
pub const F_ACK: u16 = 4;
/// Replace an existing object.
pub const F_REPLACE: u16 = 0x100;
/// Fail if the object exists.
pub const F_EXCL: u16 = 0x200;
/// Create the object.
pub const F_CREATE: u16 = 0x400;
/// Dump all objects.
pub const F_DUMP: u16 = 0x300;
/// Attribute flag: nested.
const NLA_F_NESTED: u16 = 0x8000;
/// Attribute type mask.
const NLA_TYPE_MASK: u16 = 0x3fff;

fn align(n: usize) -> usize {
    (n + 3) & !3
}

/// Builds one netlink message.
#[derive(Debug)]
pub struct Builder {
    buf: Vec<u8>,
}

impl Builder {
    /// A message of type `kind` with `flags` (the sequence number is set
    /// when it is sent).
    pub fn new(kind: u16, flags: u16) -> Builder {
        let mut buf = vec![0u8; HEADER];
        buf[4..6].copy_from_slice(&kind.to_ne_bytes());
        buf[6..8].copy_from_slice(&flags.to_ne_bytes());
        Builder { buf }
    }

    /// Appends fixed bytes (a family header such as `genlmsghdr`).
    pub fn raw(mut self, bytes: &[u8]) -> Builder {
        self.buf.extend_from_slice(bytes);
        self.pad();
        self
    }

    fn pad(&mut self) {
        let n = align(self.buf.len());
        self.buf.resize(n, 0);
    }

    /// An attribute with `value`.
    pub fn attr(mut self, kind: u16, value: &[u8]) -> Builder {
        let len = (4 + value.len()) as u16;
        self.buf.extend_from_slice(&len.to_ne_bytes());
        self.buf.extend_from_slice(&kind.to_ne_bytes());
        self.buf.extend_from_slice(value);
        self.pad();
        self
    }

    /// A `u32` attribute.
    pub fn u32(self, kind: u16, v: u32) -> Builder {
        self.attr(kind, &v.to_ne_bytes())
    }

    /// A `u16` attribute.
    pub fn u16(self, kind: u16, v: u16) -> Builder {
        self.attr(kind, &v.to_ne_bytes())
    }

    /// A `u8` attribute.
    pub fn u8(self, kind: u16, v: u8) -> Builder {
        self.attr(kind, &[v])
    }

    /// A flag attribute (no value).
    pub fn flag(self, kind: u16) -> Builder {
        self.attr(kind, &[])
    }

    /// A string attribute (NUL-terminated).
    pub fn string(self, kind: u16, s: &str) -> Builder {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        self.attr(kind, &v)
    }

    /// A nested attribute holding what `inner` adds.
    pub fn nest(mut self, kind: u16, inner: impl FnOnce(Builder) -> Builder) -> Builder {
        let start = self.buf.len();
        self.buf.extend_from_slice(&[0, 0]);
        self.buf
            .extend_from_slice(&(kind | NLA_F_NESTED).to_ne_bytes());
        let mut b = inner(self);
        let len = (b.buf.len() - start) as u16;
        b.buf[start..start + 2].copy_from_slice(&len.to_ne_bytes());
        b
    }

    /// The bytes with length and sequence number `seq`.
    pub fn finish(mut self, seq: u32) -> Vec<u8> {
        let len = self.buf.len() as u32;
        self.buf[0..4].copy_from_slice(&len.to_ne_bytes());
        self.buf[8..12].copy_from_slice(&seq.to_ne_bytes());
        self.buf
    }
}

/// One message read from the kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message<'a> {
    /// Message type.
    pub kind: u16,
    /// Flags.
    pub flags: u16,
    /// Sequence number.
    pub seq: u32,
    /// The bytes after the header.
    pub payload: &'a [u8],
}

fn ne16(b: &[u8]) -> u16 {
    u16::from_ne_bytes([b[0], b[1]])
}

fn ne32(b: &[u8]) -> u32 {
    u32::from_ne_bytes([b[0], b[1], b[2], b[3]])
}

/// The messages of one datagram; a message that does not fit ends the walk
/// with an error.
pub fn messages(mut data: &[u8]) -> impl Iterator<Item = Result<Message<'_>, NetError>> {
    std::iter::from_fn(move || {
        if data.is_empty() {
            return None;
        }
        if data.len() < HEADER {
            data = &[];
            return Some(Err(NetError::Malformed));
        }
        let len = ne32(&data[0..4]) as usize;
        if len < HEADER || len > data.len() {
            data = &[];
            return Some(Err(NetError::Malformed));
        }
        let m = Message {
            kind: ne16(&data[4..6]),
            flags: ne16(&data[6..8]),
            seq: ne32(&data[8..12]),
            payload: &data[HEADER..len],
        };
        data = &data[align(len).min(data.len())..];
        Some(Ok(m))
    })
}

/// The errno of an `NLMSG_ERROR` payload (0: acknowledgement).
pub fn error_code(payload: &[u8]) -> Result<i32, NetError> {
    if payload.len() < 4 {
        return Err(NetError::Malformed);
    }
    Ok(-(ne32(payload) as i32))
}

/// The attributes of a payload (after any fixed header); stops at the first
/// attribute that does not fit.
pub fn attrs(mut data: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    std::iter::from_fn(move || {
        if data.len() < 4 {
            return None;
        }
        let len = usize::from(ne16(&data[0..2]));
        if len < 4 || len > data.len() {
            data = &[];
            return None;
        }
        let item = (ne16(&data[2..4]) & NLA_TYPE_MASK, &data[4..len]);
        data = &data[align(len).min(data.len())..];
        Some(item)
    })
}

/// The first attribute of `kind`.
pub fn find(data: &[u8], kind: u16) -> Option<&[u8]> {
    attrs(data).find(|(k, _)| *k == kind).map(|(_, v)| v)
}

/// A `u32` attribute.
pub fn get_u32(data: &[u8], kind: u16) -> Option<u32> {
    find(data, kind).filter(|v| v.len() >= 4).map(ne32)
}

/// A `u16` attribute.
pub fn get_u16(data: &[u8], kind: u16) -> Option<u16> {
    find(data, kind).filter(|v| v.len() >= 2).map(ne16)
}

/// A string attribute (up to the first NUL, UTF-8 only).
pub fn get_string(data: &[u8], kind: u16) -> Option<String> {
    let v = find(data, kind)?;
    let end = v.iter().position(|b| *b == 0).unwrap_or(v.len());
    String::from_utf8(v[..end].to_vec()).ok()
}

/// A 6-byte address attribute.
pub fn get_mac(data: &[u8], kind: u16) -> Option<[u8; 6]> {
    find(data, kind).and_then(|v| v.try_into().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_read_back() {
        let bytes = Builder::new(0x1c, F_REQUEST | F_ACK)
            .raw(&[46, 0, 0, 0])
            .u32(3, 7)
            .string(4, "wlan0")
            .nest(45, |b| b.attr(1, &[]))
            .u16(9, 0x888e)
            .u8(10, 1)
            .flag(11)
            .finish(42);
        assert_eq!(bytes.len() % 4, 0);
        let list: Vec<_> = messages(&bytes).collect();
        assert_eq!(list.len(), 1);
        let m = list[0].unwrap();
        assert_eq!((m.kind, m.flags, m.seq), (0x1c, F_REQUEST | F_ACK, 42));
        let a = &m.payload[4..];
        assert_eq!(get_u32(a, 3), Some(7));
        assert_eq!(get_string(a, 4).as_deref(), Some("wlan0"));
        let nested = find(a, 45).unwrap();
        assert_eq!(attrs(nested).collect::<Vec<_>>(), vec![(1, &[][..])]);
        assert_eq!(get_u16(a, 9), Some(0x888e));
        assert_eq!(find(a, 10), Some(&[1u8][..]));
        assert_eq!(find(a, 11), Some(&[][..]));
        assert_eq!(get_mac(a, 3), None);
        assert_eq!(get_u32(a, 99), None);
    }

    #[test]
    fn hostile_input_ends_the_walk() {
        let one = Builder::new(1, 0).u32(1, 1).finish(1);
        let mut two = one.clone();
        two.extend_from_slice(&one);
        assert_eq!(messages(&two).count(), 2);
        let mut cut = two.clone();
        cut.truncate(two.len() - 3);
        let r: Vec<_> = messages(&cut).collect();
        assert_eq!(r.len(), 2);
        assert_eq!(r[1], Err(NetError::Malformed));
        assert_eq!(messages(&[1, 2, 3]).next(), Some(Err(NetError::Malformed)));
        let mut bad = one.clone();
        bad[0] = 3;
        assert_eq!(messages(&bad).next(), Some(Err(NetError::Malformed)));
        assert_eq!(attrs(&[200, 0, 1, 0]).count(), 0);
        assert_eq!(attrs(&[2, 0, 1, 0]).count(), 0);
        assert_eq!(error_code(&[0xfe, 0xff, 0xff, 0xff]), Ok(2));
        assert_eq!(error_code(&[0]), Err(NetError::Malformed));
        let mut seed = 5u32;
        for _ in 0..5000 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let mut t = two.clone();
            let at = seed as usize % t.len();
            t[at] = (seed >> 8) as u8;
            for m in messages(&t).flatten() {
                let _ = attrs(m.payload).count();
                let _ = get_string(m.payload, 1);
            }
        }
    }
}
