//! ATT (Core Vol 3 Part F) and the GATT client steps for HID over GATT (HOGP 1.0) in
//! boot protocol: find the HID service, its characteristics, switch it to boot protocol,
//! enable the boot keyboard / boot mouse input notifications. Our own GATT server holds
//! nothing: every request of the peer is answered "request not supported" (the MTU
//! exchange with the default 23).

use super::hci::Rd;

const UUID_PRIMARY: u16 = 0x2800;
const UUID_CHARACTERISTIC: u16 = 0x2803;
const UUID_CCCD: u16 = 0x2902;
const UUID_HID: u16 = 0x1812;
const UUID_BOOT_KEYBOARD_IN: u16 = 0x2A22;
const UUID_BOOT_MOUSE_IN: u16 = 0x2A33;
const UUID_PROTOCOL_MODE: u16 = 0x2A4E;
const MAX_CHARS: usize = 64;

/// What the client reports.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Note {
    /// Ready: has a boot keyboard / boot mouse.
    Ready {
        /// Boot keyboard input enabled.
        keyboard: bool,
        /// Boot mouse input enabled.
        mouse: bool,
    },
    /// A boot keyboard report.
    Keyboard(Vec<u8>),
    /// A boot mouse report.
    Mouse(Vec<u8>),
    /// The device has no usable HID service or refused (ATT error code, 0 = none).
    Failed(u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Services,
    Chars,
    Descriptors,
    Enable,
    Ready,
    Over,
}

/// The HOGP client of one link.
pub struct Hogp {
    phase: Phase,
    hid: Option<(u16, u16)>,
    chars: Vec<(u16, u16, u16)>,
    /// (value handle, CCCD handle, is keyboard).
    targets: Vec<(u16, u16, bool)>,
    next: usize,
    keyboard: Option<u16>,
    mouse: Option<u16>,
}

fn read_by(op: u8, start: u16, end: u16, uuid: u16) -> Vec<u8> {
    let mut v = vec![op];
    for x in [start, end, uuid] {
        v.extend_from_slice(&x.to_le_bytes());
    }
    v
}

/// The answer of our (empty) server to a request of the peer; `None` for non-requests.
pub fn serve(pdu: &[u8]) -> Option<Vec<u8>> {
    let &op = pdu.first()?;
    match op {
        0x02 => Some(vec![0x03, 23, 0]),
        0x04 | 0x06 | 0x08 | 0x0A | 0x0C | 0x0E | 0x10 | 0x12 | 0x16 | 0x18 | 0x20 => {
            let h = pdu.get(1..3).map(|h| [h[0], h[1]]).unwrap_or([0, 0]);
            let code = if matches!(op, 0x04 | 0x06 | 0x08 | 0x10) {
                0x0A
            } else {
                0x06
            };
            Some(vec![0x01, op, h[0], h[1], code])
        }
        0x1D => Some(vec![0x1E]),
        _ => None,
    }
}

impl Hogp {
    /// A client; the first request to send.
    pub fn start() -> (Self, Vec<u8>) {
        let me = Self {
            phase: Phase::Services,
            hid: None,
            chars: Vec::new(),
            targets: Vec::new(),
            next: 0,
            keyboard: None,
            mouse: None,
        };
        (me, read_by(0x10, 1, 0xFFFF, UUID_PRIMARY))
    }

    /// Ready to deliver input.
    pub fn ready(&self) -> bool {
        self.phase == Phase::Ready
    }

    /// A PDU from the peer (responses, notifications): what to send, what happened.
    pub fn pdu(&mut self, pdu: &[u8]) -> (Vec<Vec<u8>>, Vec<Note>) {
        let Some((&op, body)) = pdu.split_first() else {
            return (vec![], vec![]);
        };
        if op == 0x1B {
            return (vec![], self.notification(body));
        }
        if let Some(answer) = serve(pdu) {
            return (vec![answer], vec![]);
        }
        let step = match (op, self.phase) {
            (0x11, Phase::Services) => self.services(body),
            (0x09, Phase::Chars) => self.chars(body),
            (0x05, Phase::Descriptors) => self.descriptors(body),
            (0x13, Phase::Enable) => self.enabled(),
            (0x01, _) => self.error(body),
            _ => Err(0),
        };
        match step {
            Ok((send, notes)) => (send, notes),
            Err(code) => {
                self.phase = Phase::Over;
                (vec![], vec![Note::Failed(code)])
            }
        }
    }

    fn notification(&self, body: &[u8]) -> Vec<Note> {
        if self.phase != Phase::Ready || body.len() < 2 {
            return vec![];
        }
        let h = u16::from_le_bytes([body[0], body[1]]);
        let value = body[2..].to_vec();
        if Some(h) == self.keyboard {
            vec![Note::Keyboard(value)]
        } else if Some(h) == self.mouse {
            vec![Note::Mouse(value)]
        } else {
            vec![]
        }
    }

    fn services(&mut self, body: &[u8]) -> Result<(Vec<Vec<u8>>, Vec<Note>), u8> {
        let mut r = Rd(body);
        let len = usize::from(r.u8().map_err(|_| 0u8)?);
        if len != 6 && len != 20 {
            return Err(0);
        }
        let mut last = 0u16;
        while let Ok(e) = r.take(len) {
            let (s, end) = (
                u16::from_le_bytes([e[0], e[1]]),
                u16::from_le_bytes([e[2], e[3]]),
            );
            if end < s || s <= last {
                return Err(0);
            }
            if len == 6 && u16::from_le_bytes([e[4], e[5]]) == UUID_HID && self.hid.is_none() {
                self.hid = Some((s, end));
            }
            last = end;
        }
        if self.hid.is_some() || last == 0xFFFF {
            return self.begin_chars();
        }
        Ok((vec![read_by(0x10, last + 1, 0xFFFF, UUID_PRIMARY)], vec![]))
    }

    fn begin_chars(&mut self) -> Result<(Vec<Vec<u8>>, Vec<Note>), u8> {
        let (s, e) = self.hid.ok_or(0u8)?;
        self.phase = Phase::Chars;
        Ok((vec![read_by(0x08, s, e, UUID_CHARACTERISTIC)], vec![]))
    }

    fn chars(&mut self, body: &[u8]) -> Result<(Vec<Vec<u8>>, Vec<Note>), u8> {
        let (_, end) = self.hid.ok_or(0u8)?;
        let mut r = Rd(body);
        let len = usize::from(r.u8().map_err(|_| 0u8)?);
        if len != 7 && len != 21 {
            return Err(0);
        }
        let mut last = self.chars.last().map(|c| c.0).unwrap_or(0);
        while let Ok(e) = r.take(len) {
            let decl = u16::from_le_bytes([e[0], e[1]]);
            let value = u16::from_le_bytes([e[3], e[4]]);
            if decl <= last || self.chars.len() >= MAX_CHARS {
                return Err(0);
            }
            let uuid = if len == 7 {
                u16::from_le_bytes([e[5], e[6]])
            } else {
                0
            };
            self.chars.push((decl, value, uuid));
            last = decl;
        }
        if last >= end {
            return self.begin_descriptors();
        }
        Ok((
            vec![read_by(0x08, last + 1, end, UUID_CHARACTERISTIC)],
            vec![],
        ))
    }

    fn begin_descriptors(&mut self) -> Result<(Vec<Vec<u8>>, Vec<Note>), u8> {
        let find = |uuid| self.chars.iter().find(|c| c.2 == uuid).map(|c| c.1);
        self.keyboard = find(UUID_BOOT_KEYBOARD_IN);
        self.mouse = find(UUID_BOOT_MOUSE_IN);
        let mut send = Vec::new();
        if let Some(h) = find(UUID_PROTOCOL_MODE) {
            let mut w = vec![0x52];
            w.extend_from_slice(&h.to_le_bytes());
            w.push(0); // boot protocol
            send.push(w);
        }
        self.targets = [(self.keyboard, true), (self.mouse, false)]
            .into_iter()
            .filter_map(|(h, k)| h.map(|h| (h, 0, k)))
            .collect();
        if self.targets.is_empty() {
            return Err(0);
        }
        self.next = 0;
        send.push(self.find_info()?);
        self.phase = Phase::Descriptors;
        Ok((send, vec![]))
    }

    fn find_info(&self) -> Result<Vec<u8>, u8> {
        let (value, _, _) = *self.targets.get(self.next).ok_or(0u8)?;
        let (_, hid_end) = self.hid.ok_or(0u8)?;
        let end = self
            .chars
            .iter()
            .map(|c| c.0)
            .filter(|&d| d > value)
            .min()
            .map(|d| d - 1)
            .unwrap_or(hid_end);
        let mut v = vec![0x04];
        v.extend_from_slice(&value.saturating_add(1).to_le_bytes());
        v.extend_from_slice(&end.to_le_bytes());
        Ok(v)
    }

    fn descriptors(&mut self, body: &[u8]) -> Result<(Vec<Vec<u8>>, Vec<Note>), u8> {
        let mut r = Rd(body);
        if r.u8().map_err(|_| 0u8)? != 1 {
            return Err(0);
        }
        while let Ok(e) = r.take(4) {
            if u16::from_le_bytes([e[2], e[3]]) == UUID_CCCD {
                let cccd = u16::from_le_bytes([e[0], e[1]]);
                let t = self.targets.get_mut(self.next).ok_or(0u8)?;
                t.1 = cccd;
                let mut w = vec![0x12];
                w.extend_from_slice(&cccd.to_le_bytes());
                w.extend_from_slice(&[0x01, 0x00]);
                self.phase = Phase::Enable;
                return Ok((vec![w], vec![]));
            }
        }
        Err(0)
    }

    fn enabled(&mut self) -> Result<(Vec<Vec<u8>>, Vec<Note>), u8> {
        self.next += 1;
        if self.next < self.targets.len() {
            self.phase = Phase::Descriptors;
            return Ok((vec![self.find_info()?], vec![]));
        }
        self.phase = Phase::Ready;
        let note = Note::Ready {
            keyboard: self.keyboard.is_some(),
            mouse: self.mouse.is_some(),
        };
        Ok((vec![], vec![note]))
    }

    fn error(&mut self, body: &[u8]) -> Result<(Vec<Vec<u8>>, Vec<Note>), u8> {
        let (req, code) = match body {
            [req, _, _, code, ..] => (*req, *code),
            _ => return Err(0),
        };
        match (req, code, self.phase) {
            (0x10, 0x0A, Phase::Services) if self.hid.is_some() => self.begin_chars(),
            (0x08, 0x0A, Phase::Chars) if !self.chars.is_empty() => self.begin_descriptors(),
            _ => Err(code),
        }
    }
}

#[cfg(test)]
#[path = "att_tests.rs"]
mod tests;
