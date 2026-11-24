//! HCI packets (Core Vol 4 Part E) in H4 framing (one type octet first) – what both the
//! Linux HCI user channel and a UART controller carry. Every input is hostile: every
//! length is checked, nothing is trusted to be in range, unknown events are kept as such.

/// H4 packet types.
pub const H4_CMD: u8 = 0x01;
/// ACL data.
pub const H4_ACL: u8 = 0x02;
/// Event.
pub const H4_EVT: u8 = 0x04;

/// A device address in wire order (little-endian) plus whether it is an LE random one.
#[derive(
    Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct Addr {
    /// The six octets, least significant first.
    pub bytes: [u8; 6],
    /// LE random address (else public / BR/EDR).
    pub random: bool,
}

impl Addr {
    /// A public address from wire octets.
    pub const fn public(bytes: [u8; 6]) -> Self {
        Self {
            bytes,
            random: false,
        }
    }

    /// `AA:BB:CC:DD:EE:FF` (most significant first), `/r` appended for random.
    pub fn text(&self) -> String {
        let b = &self.bytes;
        let mut s = format!(
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            b[5], b[4], b[3], b[2], b[1], b[0]
        );
        if self.random {
            s.push_str("/r");
        }
        s
    }

    /// Parses [`Addr::text`]; anything else is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let (hex, random) = match text.strip_suffix("/r") {
            Some(h) => (h, true),
            None => (text, false),
        };
        let parts: Vec<&str> = hex.split(':').collect();
        if parts.len() != 6 || hex.len() != 17 {
            return None;
        }
        let mut bytes = [0u8; 6];
        for (i, p) in parts.iter().enumerate() {
            if p.len() != 2 {
                return None;
            }
            bytes[5 - i] = u8::from_str_radix(p, 16).ok()?;
        }
        Some(Self { bytes, random })
    }

    /// The 7-octet `A1`/`A2` value of `f5`/`f6` (big-endian: type, then address).
    pub fn smp_value(&self) -> [u8; 7] {
        let mut out = [0u8; 7];
        out[0] = u8::from(self.random);
        for i in 0..6 {
            out[1 + i] = self.bytes[5 - i];
        }
        out
    }
}

/// Command opcodes (OGF << 10 | OCF).
pub mod op {
    #![allow(missing_docs)]
    pub const INQUIRY: u16 = 0x0401;
    pub const INQUIRY_CANCEL: u16 = 0x0402;
    pub const CREATE_CONNECTION: u16 = 0x0405;
    pub const DISCONNECT: u16 = 0x0406;
    pub const ACCEPT_CONNECTION: u16 = 0x0409;
    pub const REJECT_CONNECTION: u16 = 0x040A;
    pub const LINK_KEY_REPLY: u16 = 0x040B;
    pub const LINK_KEY_NEG_REPLY: u16 = 0x040C;
    pub const PIN_CODE_NEG_REPLY: u16 = 0x040E;
    pub const AUTH_REQUESTED: u16 = 0x0411;
    pub const SET_CONN_ENCRYPTION: u16 = 0x0413;
    pub const REMOTE_NAME_REQUEST: u16 = 0x0419;
    pub const REMOTE_NAME_REQUEST_CANCEL: u16 = 0x041A;
    pub const IO_CAPABILITY_REPLY: u16 = 0x042B;
    pub const USER_CONFIRM_REPLY: u16 = 0x042C;
    pub const USER_CONFIRM_NEG_REPLY: u16 = 0x042D;
    pub const USER_PASSKEY_NEG_REPLY: u16 = 0x042F;
    pub const IO_CAPABILITY_NEG_REPLY: u16 = 0x0434;
    pub const SET_EVENT_MASK: u16 = 0x0C01;
    pub const RESET: u16 = 0x0C03;
    pub const WRITE_SCAN_ENABLE: u16 = 0x0C1A;
    pub const WRITE_INQUIRY_MODE: u16 = 0x0C45;
    pub const WRITE_SSP_MODE: u16 = 0x0C56;
    pub const WRITE_LE_HOST_SUPPORTED: u16 = 0x0C6D;
    pub const WRITE_SC_HOST_SUPPORT: u16 = 0x0C7A;
    pub const READ_LOCAL_VERSION: u16 = 0x1001;
    pub const READ_BD_ADDR: u16 = 0x1009;
    pub const READ_BUFFER_SIZE: u16 = 0x1005;
    pub const LE_SET_EVENT_MASK: u16 = 0x2001;
    pub const LE_READ_BUFFER_SIZE: u16 = 0x2002;
    pub const LE_SET_ADV_PARAMS: u16 = 0x2006;
    pub const LE_SET_ADV_DATA: u16 = 0x2008;
    pub const LE_SET_ADV_ENABLE: u16 = 0x200A;
    pub const LE_SET_SCAN_PARAMS: u16 = 0x200B;
    pub const LE_SET_SCAN_ENABLE: u16 = 0x200C;
    pub const LE_CREATE_CONNECTION: u16 = 0x200D;
    pub const LE_CREATE_CONNECTION_CANCEL: u16 = 0x200E;
    pub const LE_START_ENCRYPTION: u16 = 0x2019;
    pub const LE_LTK_REPLY: u16 = 0x201A;
    pub const LE_LTK_NEG_REPLY: u16 = 0x201B;
}

/// A command in H4 framing.
pub fn command(opcode: u16, params: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + params.len());
    let [lo, hi] = opcode.to_le_bytes();
    out.extend_from_slice(&[H4_CMD, lo, hi, params.len().min(255) as u8]);
    out.extend_from_slice(&params[..params.len().min(255)]);
    out
}

/// One ACL fragment in H4 framing (`first`: start of an L2CAP frame).
pub fn acl(handle: u16, first: bool, data: &[u8]) -> Vec<u8> {
    let pb: u16 = if first { 0x2000 } else { 0x1000 };
    let h = (handle & 0x0fff) | pb;
    let mut out = Vec::with_capacity(5 + data.len());
    out.push(H4_ACL);
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(data);
    out
}

/// A malformed packet (dropped, counted, never acted on).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Malformed;

/// A little reader that fails closed on every short read.
pub struct Rd<'a>(pub &'a [u8]);

impl<'a> Rd<'a> {
    /// Takes `n` octets.
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], Malformed> {
        if self.0.len() < n {
            return Err(Malformed);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    /// One octet.
    pub fn u8(&mut self) -> Result<u8, Malformed> {
        Ok(self.take(1)?[0])
    }
    /// Little-endian u16.
    pub fn u16(&mut self) -> Result<u16, Malformed> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    /// Little-endian u24 (class of device).
    pub fn u24(&mut self) -> Result<u32, Malformed> {
        let b = self.take(3)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], 0]))
    }
    /// Little-endian u32.
    pub fn u32(&mut self) -> Result<u32, Malformed> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    /// Fixed array.
    pub fn arr<const N: usize>(&mut self) -> Result<[u8; N], Malformed> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }
    /// A public address.
    pub fn addr(&mut self) -> Result<Addr, Malformed> {
        Ok(Addr::public(self.arr()?))
    }
}

/// A device seen during inquiry or an LE scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    /// Address.
    pub addr: Addr,
    /// Class of device (BR/EDR), 0 for LE.
    pub class: u32,
    /// RSSI in dBm, if reported.
    pub rssi: Option<i8>,
    /// EIR / advertising data (TLV), possibly empty.
    pub data: Vec<u8>,
    /// LE (else BR/EDR).
    pub le: bool,
    /// LE: connectable advertising.
    pub connectable: bool,
    /// LE: a scan response (to an active scan), not an advertisement.
    pub scan_response: bool,
    /// BR/EDR: page scan repetition mode and clock offset (for the remote name request).
    pub page: (u8, u16),
}

/// The events this host acts on.
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    CommandComplete {
        opcode: u16,
        status: u8,
        data: Vec<u8>,
    },
    CommandStatus {
        opcode: u16,
        status: u8,
    },
    InquiryComplete,
    Seen(Vec<Seen>),
    ConnectionComplete {
        status: u8,
        handle: u16,
        addr: Addr,
        acl: bool,
    },
    ConnectionRequest {
        addr: Addr,
        class: u32,
        acl: bool,
    },
    Disconnected {
        handle: u16,
        reason: u8,
    },
    RemoteName {
        status: u8,
        addr: Addr,
        name: Vec<u8>,
    },
    AuthComplete {
        status: u8,
        handle: u16,
    },
    EncryptionChange {
        status: u8,
        handle: u16,
        on: bool,
    },
    CompletedPackets(Vec<(u16, u16)>),
    PinCodeRequest(Addr),
    LinkKeyRequest(Addr),
    LinkKeyNotification {
        addr: Addr,
        key: [u8; 16],
        kind: u8,
    },
    IoCapRequest(Addr),
    IoCapResponse {
        addr: Addr,
        io: u8,
        auth: u8,
    },
    UserConfirm {
        addr: Addr,
        value: u32,
    },
    UserPasskeyRequest(Addr),
    PasskeyNotification {
        addr: Addr,
        passkey: u32,
    },
    SspComplete {
        status: u8,
        addr: Addr,
    },
    LeConnected {
        status: u8,
        handle: u16,
        central: bool,
        addr: Addr,
    },
    LeLtkRequest {
        handle: u16,
        rand: [u8; 8],
        ediv: u16,
    },
    Other(u8),
}

/// Parses an event (without the H4 octet).
pub fn event(p: &[u8]) -> Result<Event, Malformed> {
    let mut r = Rd(p);
    let code = r.u8()?;
    let len = usize::from(r.u8()?);
    let mut r = Rd(r.take(len)?);
    Ok(match code {
        0x0E => {
            let _credits = r.u8()?;
            let opcode = r.u16()?;
            let status = if r.0.is_empty() { 0 } else { r.u8()? };
            Event::CommandComplete {
                opcode,
                status,
                data: r.0.to_vec(),
            }
        }
        0x0F => {
            let status = r.u8()?;
            let _credits = r.u8()?;
            Event::CommandStatus {
                opcode: r.u16()?,
                status,
            }
        }
        0x01..=0x08 | 0x13 | 0x22 | 0x2F => link_event(code, &mut r)?,
        0x16 => Event::PinCodeRequest(r.addr()?),
        0x17 => Event::LinkKeyRequest(r.addr()?),
        0x18 => Event::LinkKeyNotification {
            addr: r.addr()?,
            key: r.arr()?,
            kind: r.u8()?,
        },
        0x31 => Event::IoCapRequest(r.addr()?),
        0x32 => {
            let (addr, io, _oob) = (r.addr()?, r.u8()?, r.u8()?);
            Event::IoCapResponse {
                addr,
                io,
                auth: r.u8()?,
            }
        }
        0x33 => Event::UserConfirm {
            addr: r.addr()?,
            value: r.u32()?,
        },
        0x34 => Event::UserPasskeyRequest(r.addr()?),
        0x3B => Event::PasskeyNotification {
            addr: r.addr()?,
            passkey: r.u32()?,
        },
        0x36 => Event::SspComplete {
            status: r.u8()?,
            addr: r.addr()?,
        },
        0x3E => le_meta(&mut r)?,
        other => Event::Other(other),
    })
}

/// Inquiry and connection events.
fn link_event(code: u8, r: &mut Rd<'_>) -> Result<Event, Malformed> {
    Ok(match code {
        0x01 => Event::InquiryComplete,
        0x02 | 0x22 | 0x2F => Event::Seen(inquiry(code, r)?),
        0x03 => {
            let (status, handle, addr) = (r.u8()?, r.u16()?, r.addr()?);
            Event::ConnectionComplete {
                status,
                handle,
                addr,
                acl: r.u8()? == 1,
            }
        }
        0x04 => Event::ConnectionRequest {
            addr: r.addr()?,
            class: r.u24()?,
            acl: r.u8()? == 1,
        },
        0x05 => {
            let _status = r.u8()?;
            Event::Disconnected {
                handle: r.u16()?,
                reason: r.u8()?,
            }
        }
        0x06 => Event::AuthComplete {
            status: r.u8()?,
            handle: r.u16()?,
        },
        0x07 => {
            let (status, addr) = (r.u8()?, r.addr()?);
            let raw = r.take(r.0.len().min(248))?;
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            Event::RemoteName {
                status,
                addr,
                name: raw[..end].to_vec(),
            }
        }
        0x08 => Event::EncryptionChange {
            status: r.u8()?,
            handle: r.u16()?,
            on: r.u8()? != 0,
        },
        0x13 => {
            let n = r.u8()?;
            let mut v = Vec::new();
            for _ in 0..n {
                v.push((r.u16()? & 0x0fff, r.u16()?));
            }
            Event::CompletedPackets(v)
        }
        _ => Event::Other(code),
    })
}

/// The responses of an inquiry result event. Each response is packed after the previous
/// one (what controllers send and BlueZ reads); `0x22` comes in two layouts, with and without
/// the page scan mode octet (14 or 15 octets per response), told apart by the length.
fn inquiry(code: u8, r: &mut Rd<'_>) -> Result<Vec<Seen>, Malformed> {
    let n = usize::from(r.u8()?);
    if n == 0 {
        return Ok(Vec::new());
    }
    let rssi_pscan = code == 0x22 && r.0.len() / n == 15;
    let mut out = Vec::new();
    for _ in 0..n.min(if code == 0x2F { 1 } else { 255 }) {
        let addr = r.addr()?;
        let psrm = r.u8()?;
        let skip = match code {
            0x02 => 2,
            0x22 if rssi_pscan => 2,
            _ => 1,
        };
        r.take(skip)?;
        let class = r.u24()?;
        let clock = r.u16()?;
        let rssi = if code == 0x02 {
            None
        } else {
            Some(r.u8()? as i8)
        };
        let data = if code == 0x2F {
            r.take(240.min(r.0.len()))?.to_vec()
        } else {
            Vec::new()
        };
        out.push(Seen {
            addr,
            class,
            rssi: rssi.filter(|&v| v != 127),
            data,
            le: false,
            connectable: true,
            scan_response: false,
            page: (psrm, clock & 0x7fff),
        });
    }
    Ok(out)
}

fn le_meta(r: &mut Rd<'_>) -> Result<Event, Malformed> {
    Ok(match r.u8()? {
        0x01 | 0x0A => {
            let (status, handle, role, kind) = (r.u8()?, r.u16()?, r.u8()?, r.u8()?);
            let bytes = r.arr()?;
            let addr = Addr {
                bytes,
                random: kind & 1 == 1,
            };
            Event::LeConnected {
                status,
                handle,
                central: role == 0,
                addr,
            }
        }
        0x02 => {
            let n = r.u8()?;
            let mut out = Vec::new();
            for _ in 0..n {
                let (kind, atype) = (r.u8()?, r.u8()?);
                let bytes = r.arr()?;
                let len = usize::from(r.u8()?);
                let data = r.take(len.min(31))?.to_vec();
                if len > 31 {
                    return Err(Malformed);
                }
                let rssi = r.u8()? as i8;
                out.push(Seen {
                    addr: Addr {
                        bytes,
                        random: atype & 1 == 1,
                    },
                    class: 0,
                    rssi: (rssi != 127).then_some(rssi),
                    data,
                    le: true,
                    connectable: kind <= 0x01,
                    scan_response: kind == 0x04,
                    page: (0, 0),
                });
            }
            Event::Seen(out)
        }
        0x05 => Event::LeLtkRequest {
            handle: r.u16()?,
            rand: r.arr()?,
            ediv: r.u16()?,
        },
        _ => Event::Other(0x3E),
    })
}

/// An ACL fragment (without the H4 octet): (handle, starts a frame, payload).
pub fn acl_in(p: &[u8]) -> Result<(u16, bool, &[u8]), Malformed> {
    let mut r = Rd(p);
    let h = r.u16()?;
    let len = usize::from(r.u16()?);
    let data = r.take(len)?;
    let pb = (h >> 12) & 0x3;
    Ok((h & 0x0fff, pb != 0x1, data))
}

/// What EIR / advertising data says (TLV records).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Adv {
    /// Name (lossy UTF-8, control characters removed, at most 64 octets).
    pub name: Option<String>,
    /// The name is the complete one (0x09), not a shortened one (0x08).
    pub name_complete: bool,
    /// LE appearance.
    pub appearance: Option<u16>,
    /// The HID service (0x1812) is listed.
    pub hid: bool,
}

/// Parses EIR / AD records; a broken record ends the walk.
pub fn adv(data: &[u8]) -> Adv {
    let mut out = Adv::default();
    let mut r = Rd(data);
    while let Ok(len) = r.u8() {
        if len == 0 {
            break;
        }
        let Ok(rec) = r.take(usize::from(len)) else {
            break;
        };
        let (kind, v) = (rec[0], &rec[1..]);
        match kind {
            0x08 | 0x09 if out.name.is_none() || kind == 0x09 => {
                out.name = Some(clean_name(v));
                out.name_complete = kind == 0x09;
            }
            0x19 if v.len() == 2 => out.appearance = Some(u16::from_le_bytes([v[0], v[1]])),
            0x02 | 0x03 => out.hid |= v.chunks_exact(2).any(|c| c == [0x12, 0x18]),
            _ => {}
        }
    }
    out
}

/// A device name as data: lossy UTF-8, no control characters, ≤ 64 octets.
pub fn clean_name(raw: &[u8]) -> String {
    let mut s: String = String::from_utf8_lossy(raw)
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    while s.len() > 64 {
        s.pop();
    }
    s
}

#[cfg(test)]
#[path = "hci_tests.rs"]
mod tests;
