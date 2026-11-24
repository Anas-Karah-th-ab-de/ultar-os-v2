//! L2CAP (Core Vol 3 Part A), basic mode only: reassembly of ACL fragments with a hard
//! frame limit, the fixed LE channels (ATT, LE signalling, SMP) and – for classic HID –
//! the BR/EDR signalling channel with connection-oriented channels (connect, configure
//! without options, disconnect). Incoming channels are refused unless the caller allows
//! the PSM (default deny).

use super::hci::{self, Malformed, Rd};

/// BR/EDR signalling channel.
pub const SIGNAL: u16 = 0x0001;
/// Attribute protocol.
pub const ATT: u16 = 0x0004;
/// LE signalling channel.
pub const LE_SIGNAL: u16 = 0x0005;
/// Security manager.
pub const SMP: u16 = 0x0006;
/// The longest L2CAP frame accepted (payload); longer frames are dropped whole.
pub const MAX_FRAME: usize = 1024;
/// HID control / interrupt PSMs.
pub const PSM_HID_CONTROL: u16 = 0x0011;
/// HID interrupt channel.
pub const PSM_HID_INTERRUPT: u16 = 0x0013;
/// Open channels per link at most.
pub const MAX_CHANNELS: usize = 4;

/// Reassembles the ACL fragments of one link.
#[derive(Default)]
pub struct Reassembly {
    buf: Vec<u8>,
    want: usize,
}

impl Reassembly {
    /// One fragment; a whole frame comes back as (channel id, payload).
    pub fn push(&mut self, first: bool, data: &[u8]) -> Option<Result<(u16, Vec<u8>), Malformed>> {
        if first {
            self.buf.clear();
            let mut r = Rd(data);
            let (Ok(len), Ok(_cid)) = (r.u16(), r.u16()) else {
                return Some(Err(Malformed));
            };
            if usize::from(len) > MAX_FRAME {
                self.want = 0;
                return Some(Err(Malformed));
            }
            self.want = usize::from(len) + 4;
        } else if self.want == 0 {
            return Some(Err(Malformed));
        }
        if self.buf.len() + data.len() > self.want {
            self.buf.clear();
            self.want = 0;
            return Some(Err(Malformed));
        }
        self.buf.extend_from_slice(data);
        if self.buf.len() < self.want {
            return None;
        }
        self.want = 0;
        let cid = u16::from_le_bytes([self.buf[2], self.buf[3]]);
        Some(Ok((cid, self.buf.split_off(4))))
    }
}

/// A frame cut into H4 ACL packets of at most `mtu` octets.
pub fn send(handle: u16, cid: u16, payload: &[u8], mtu: usize) -> Vec<Vec<u8>> {
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    frame.extend_from_slice(&cid.to_le_bytes());
    frame.extend_from_slice(payload);
    frame
        .chunks(mtu.max(23))
        .enumerate()
        .map(|(i, c)| hci::acl(handle, i == 0, c))
        .collect()
}

/// A connection-oriented channel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Channel {
    /// Protocol.
    pub psm: u16,
    /// Our channel id.
    pub local: u16,
    /// The peer's channel id (0 while connecting).
    pub remote: u16,
    our_conf: bool,
    their_conf: bool,
}

impl Channel {
    /// Both configurations done.
    pub fn open(&self) -> bool {
        self.our_conf && self.their_conf && self.remote != 0
    }
}

/// What signalling did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SigEvent {
    /// A channel became usable.
    Open(Channel),
    /// A channel closed (or its connection was refused).
    Closed(u16),
}

/// The BR/EDR signalling of one link.
#[derive(Default)]
pub struct Signaling {
    next_id: u8,
    next_cid: u16,
    /// The channels.
    pub channels: Vec<Channel>,
}

fn sig(code: u8, id: u8, data: &[u8]) -> Vec<u8> {
    let mut out = vec![code, id];
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(data);
    out
}

fn le16(parts: &[u16]) -> Vec<u8> {
    parts.iter().flat_map(|v| v.to_le_bytes()).collect()
}

impl Signaling {
    fn id(&mut self) -> u8 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.next_id
    }

    fn cid(&mut self) -> Option<u16> {
        if self.channels.len() >= MAX_CHANNELS {
            return None;
        }
        self.next_cid = self.next_cid.max(0x0040).wrapping_add(1).max(0x0041);
        Some(self.next_cid)
    }

    /// Opens a channel to `psm`: the signalling PDU to send.
    pub fn connect(&mut self, psm: u16) -> Option<Vec<u8>> {
        let local = self.cid()?;
        self.channels.push(Channel {
            psm,
            local,
            remote: 0,
            our_conf: false,
            their_conf: false,
        });
        let id = self.id();
        Some(sig(0x02, id, &le16(&[psm, local])))
    }

    /// Closes a channel: the PDU to send.
    pub fn disconnect(&mut self, local: u16) -> Option<Vec<u8>> {
        let ch = self.channels.iter().find(|c| c.local == local).copied()?;
        let id = self.id();
        Some(sig(0x06, id, &le16(&[ch.remote, ch.local])))
    }

    /// A channel by its id.
    pub fn by_local(&self, local: u16) -> Option<&Channel> {
        self.channels.iter().find(|c| c.local == local)
    }

    /// One signalling frame (may hold several commands): answers and events.
    pub fn frame(
        &mut self,
        mut data: &[u8],
        allow: &dyn Fn(u16) -> bool,
    ) -> (Vec<Vec<u8>>, Vec<SigEvent>) {
        let (mut out, mut ev) = (Vec::new(), Vec::new());
        while data.len() >= 4 {
            let (code, id) = (data[0], data[1]);
            let len = usize::from(u16::from_le_bytes([data[2], data[3]]));
            let Some(body) = data.get(4..4 + len) else {
                break;
            };
            self.command(code, id, body, allow, &mut out, &mut ev);
            data = &data[4 + len..];
        }
        (out, ev)
    }

    fn command(
        &mut self,
        code: u8,
        id: u8,
        body: &[u8],
        allow: &dyn Fn(u16) -> bool,
        out: &mut Vec<Vec<u8>>,
        ev: &mut Vec<SigEvent>,
    ) {
        let mut r = Rd(body);
        match code {
            0x02 => {
                if let (Ok(psm), Ok(scid)) = (r.u16(), r.u16()) {
                    self.connection_request(id, psm, scid, allow, out);
                }
            }
            0x03 => {
                if let (Ok(dcid), Ok(scid), Ok(result)) = (r.u16(), r.u16(), r.u16()) {
                    self.connection_response(dcid, scid, result, out, ev);
                }
            }
            0x04 => {
                if let Ok(dcid) = r.u16() {
                    self.configure_request(id, dcid, out, ev);
                }
            }
            0x05 => {
                if let (Ok(scid), Ok(_flags), Ok(result)) = (r.u16(), r.u16(), r.u16()) {
                    self.configure_response(scid, result, out, ev);
                }
            }
            0x06 => {
                if let (Ok(dcid), Ok(scid)) = (r.u16(), r.u16()) {
                    self.closed(dcid, ev);
                    out.push(sig(0x07, id, &le16(&[dcid, scid])));
                }
            }
            0x07 => {
                if let (Ok(_dcid), Ok(scid)) = (r.u16(), r.u16()) {
                    self.closed(scid, ev);
                }
            }
            0x0A => {
                if let Ok(kind) = r.u16() {
                    out.push(sig(0x0B, id, &le16(&[kind, 0x0001])));
                }
            }
            0x01 | 0x0B => {}
            _ => out.push(sig(0x01, id, &le16(&[0x0000]))),
        }
    }

    fn closed(&mut self, local: u16, ev: &mut Vec<SigEvent>) {
        if let Some(i) = self.channels.iter().position(|c| c.local == local) {
            self.channels.remove(i);
            ev.push(SigEvent::Closed(local));
        }
    }

    fn connection_request(
        &mut self,
        id: u8,
        psm: u16,
        scid: u16,
        allow: &dyn Fn(u16) -> bool,
        out: &mut Vec<Vec<u8>>,
    ) {
        let local = if allow(psm) { self.cid() } else { None };
        let Some(local) = local else {
            // 0x0003: security block (refused).
            out.push(sig(0x03, id, &le16(&[0, scid, 0x0003, 0])));
            return;
        };
        self.channels.push(Channel {
            psm,
            local,
            remote: scid,
            our_conf: false,
            their_conf: false,
        });
        out.push(sig(0x03, id, &le16(&[local, scid, 0, 0])));
        let cid = self.id();
        out.push(sig(0x04, cid, &le16(&[scid, 0])));
    }

    fn connection_response(
        &mut self,
        dcid: u16,
        scid: u16,
        result: u16,
        out: &mut Vec<Vec<u8>>,
        ev: &mut Vec<SigEvent>,
    ) {
        if result == 0x0001 {
            return; // pending
        }
        let Some(i) = self.channels.iter().position(|c| c.local == scid) else {
            return;
        };
        if result != 0 || dcid < 0x0040 {
            self.channels.remove(i);
            ev.push(SigEvent::Closed(scid));
            return;
        }
        self.channels[i].remote = dcid;
        let cid = self.id();
        out.push(sig(0x04, cid, &le16(&[dcid, 0])));
    }

    fn configure_request(
        &mut self,
        id: u8,
        dcid: u16,
        out: &mut Vec<Vec<u8>>,
        ev: &mut Vec<SigEvent>,
    ) {
        let Some(ch) = self.channels.iter_mut().find(|c| c.local == dcid) else {
            out.push(sig(0x01, id, &le16(&[0x0002, dcid, 0])));
            return;
        };
        ch.their_conf = true;
        // Accepted as offered: MTU and flush timeout are hints we can live with.
        out.push(sig(0x05, id, &le16(&[ch.remote, 0, 0])));
        if ch.open() {
            ev.push(SigEvent::Open(*ch));
        }
    }

    fn configure_response(
        &mut self,
        scid: u16,
        result: u16,
        out: &mut Vec<Vec<u8>>,
        ev: &mut Vec<SigEvent>,
    ) {
        let Some(i) = self.channels.iter().position(|c| c.local == scid) else {
            return;
        };
        if result != 0 {
            let ch = self.channels.remove(i);
            let did = self.id();
            out.push(sig(0x06, did, &le16(&[ch.remote, ch.local])));
            ev.push(SigEvent::Closed(scid));
            return;
        }
        self.channels[i].our_conf = true;
        if self.channels[i].open() {
            ev.push(SigEvent::Open(self.channels[i]));
        }
    }
}

/// LE signalling: every request is refused politely (connection parameter updates are
/// not applied in this round), unknown commands get "command not understood".
pub fn le_signal(data: &[u8]) -> Option<Vec<u8>> {
    let (&code, rest) = data.split_first()?;
    let &id = rest.first()?;
    match code {
        0x12 => Some(sig(0x13, id, &le16(&[0x0001]))),
        0x01 | 0x13 => None,
        _ => Some(sig(0x01, id, &le16(&[0x0000]))),
    }
}
