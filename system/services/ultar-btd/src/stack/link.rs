//! One connection and what runs on it: L2CAP, SMP, the HOGP client, classic HID.

use super::att::{self, Hogp, Note};
use super::bonds::{Bond, Kind};
use super::hci::{op, Addr};
use super::hid::{self, InputKind, Keyboard, Mouse};
use super::host::{Host, Up, Want, PAIR_MS};
use super::l2cap::{self, Reassembly, SigEvent, Signaling};
use super::smp::{reason, Ask, Out, Smp};

/// HID state of a link.
#[derive(Default)]
pub struct HidState {
    kb: Keyboard,
    mouse: Mouse,
    /// Input ids registered with the sink.
    pub inputs: Vec<u32>,
    ctrl: Option<u16>,
    intr: Option<u16>,
}

/// A connection.
pub struct Link {
    /// HCI handle.
    pub handle: u16,
    /// The address on air.
    pub addr: Addr,
    /// The bond identity (= `addr` unless an LE private address was resolved).
    pub id_addr: Addr,
    /// LE.
    pub le: bool,
    /// We are central.
    pub central: bool,
    /// Encrypted.
    pub encrypted: bool,
    /// LE pairing.
    pub smp: Option<Smp>,
    rx: Reassembly,
    sig: Signaling,
    hogp: Option<Hogp>,
    /// HID.
    pub hid: HidState,
    /// A pairing the person asked for: (dialog token, since).
    pub pairing: Option<(u32, u64)>,
    /// BR/EDR: the peer's IO capability.
    pub peer_io: u8,
}

impl Link {
    /// A new link.
    pub fn new(handle: u16, addr: Addr, id_addr: Addr, le: bool, central: bool) -> Self {
        Self {
            handle,
            addr,
            id_addr,
            le,
            central,
            encrypted: false,
            smp: None,
            rx: Reassembly::default(),
            sig: Signaling::default(),
            hogp: None,
            hid: HidState::default(),
            pairing: None,
            peer_io: 0xff,
        }
    }
}

fn hci_handle(h: u16) -> [u8; 2] {
    h.to_le_bytes()
}

impl Host {
    pub(crate) fn link(&mut self, handle: u16) -> Option<usize> {
        self.links.iter().position(|l| l.handle == handle)
    }

    /// Starts pairing on link `i` (the person asked).
    pub(crate) fn pair_on(&mut self, i: usize) {
        let now = self.now;
        let l = &mut self.links[i];
        l.pairing = Some((0, now));
        let h = l.handle;
        if !l.le {
            self.cmd(op::AUTH_REQUESTED, &hci_handle(h));
            return;
        }
        let mut smp = Smp::new(l.central, self.cfg.io, self.addr, l.addr);
        let outs = smp.start();
        self.links[i].smp = Some(smp);
        self.smp_outs(i, outs);
    }

    /// An ACL fragment from the controller.
    pub(crate) fn acl(&mut self, handle: u16, first: bool, data: &[u8]) {
        let Some(i) = self.link(handle) else { return };
        let Some(Ok((cid, frame))) = self.links[i].rx.push(first, data) else {
            return;
        };
        let le = self.links[i].le;
        match (le, cid) {
            (true, l2cap::SMP) => self.smp_pdu(i, &frame),
            (true, l2cap::ATT) => self.att_pdu(i, &frame),
            (true, l2cap::LE_SIGNAL) => {
                if let Some(r) = l2cap::le_signal(&frame) {
                    self.l2cap(handle, l2cap::LE_SIGNAL, &r);
                }
            }
            (false, l2cap::SIGNAL) => self.signal(i, &frame),
            (false, c) if Some(c) == self.links[i].hid.intr => self.classic_report(i, &frame),
            _ => {}
        }
    }

    fn smp_pdu(&mut self, i: usize, pdu: &[u8]) {
        let l = &self.links[i];
        let (central, h, id) = (l.central, l.handle, l.id_addr);
        if central && pdu.first() == Some(&0x0B) && l.smp.as_ref().is_none_or(|s| !s.active()) {
            // Security request: encrypt with the bond, pair only if the person asked.
            if let Some(b) = self.bonds.find(&id).filter(|b| b.le) {
                let key = b.key;
                self.start_encryption(h, &key);
            } else if self.wants(&id) == Some(Want::Pair) {
                self.pair_on(i);
            } else {
                self.l2cap(h, l2cap::SMP, &[0x05, reason::NOT_SUPPORTED]);
            }
            return;
        }
        if self.links[i].smp.is_none() && !central {
            self.links[i].smp = Some(Smp::new(false, self.cfg.io, self.addr, self.links[i].addr));
            self.links[i].pairing = Some((0, self.now));
        }
        let pairable = self.cfg.pairable_le && !central;
        let Some(smp) = self.links[i].smp.as_mut() else {
            self.l2cap(h, l2cap::SMP, &[0x05, reason::NOT_SUPPORTED]);
            return;
        };
        let outs = smp.pdu(pdu, pairable);
        self.smp_outs(i, outs);
    }

    pub(crate) fn start_encryption(&mut self, h: u16, ltk: &[u8; 16]) {
        let mut p = hci_handle(h).to_vec();
        p.extend_from_slice(&[0; 10]); // random number and EDIV: 0 for Secure Connections
        p.extend_from_slice(ltk);
        self.cmd(op::LE_START_ENCRYPTION, &p);
    }

    pub(crate) fn smp_outs(&mut self, i: usize, outs: Vec<Out>) {
        let (h, addr, id) = (
            self.links[i].handle,
            self.links[i].addr,
            self.links[i].id_addr,
        );
        for o in outs {
            match o {
                Out::Send(pdu) => self.l2cap(h, l2cap::SMP, &pdu),
                Out::Ask(ask) => self.ask(i, ask),
                Out::Encrypt(ltk) => self.start_encryption(h, &ltk),
                Out::Done(keys) => {
                    let (irk, identity) = match keys.identity {
                        Some((irk, a)) => (Some(*irk), a),
                        None => (None, id),
                    };
                    self.links[i].id_addr = identity;
                    self.links[i].pairing = None;
                    let bond = Bond {
                        addr: identity,
                        le: true,
                        key: *keys.ltk,
                        key_type: 0,
                        authenticated: keys.authenticated,
                        irk,
                        name: self.name_of(&addr),
                        kind: self.kind_of(&addr),
                    };
                    self.store_bond(bond);
                    self.finish_want(&id, 0);
                    self.finish_want(&addr, 0);
                    self.start_hogp(i);
                }
                Out::Failed(r) => {
                    self.links[i].pairing = None;
                    self.finish_want(&addr, r);
                    self.cmd(op::DISCONNECT, &[h as u8, (h >> 8) as u8, 0x05]);
                }
            }
        }
        self.up.push(Up::Changed);
    }

    pub(crate) fn ask(&mut self, i: usize, ask: Ask) {
        let token = self.token();
        let since = self.links[i].pairing.map(|p| p.1).unwrap_or(self.now);
        self.links[i].pairing = Some((token, since));
        let addr = self.links[i].addr;
        let name = self.name_of(&addr);
        self.up.push(Up::Ask {
            token,
            addr,
            name,
            ask,
        });
    }

    /// The person's answer.
    pub(crate) fn answer(&mut self, token: u32, accept: bool) {
        let Some(i) = self
            .links
            .iter()
            .position(|l| l.pairing.is_some_and(|p| p.0 == token))
        else {
            return;
        };
        let (le, addr) = (self.links[i].le, self.links[i].addr);
        if le {
            let outs = self.links[i]
                .smp
                .as_mut()
                .map(|s| s.answer(accept))
                .unwrap_or_default();
            self.smp_outs(i, outs);
        } else if self.links[i].peer_io != 0xfe {
            // 0xfe marks a passkey notification: nothing to reply, a "no" disconnects.
            let o = if accept {
                op::USER_CONFIRM_REPLY
            } else {
                op::USER_CONFIRM_NEG_REPLY
            };
            self.cmd(o, &addr.bytes);
        } else if !accept {
            let h = self.links[i].handle;
            self.cmd(op::DISCONNECT, &[h as u8, (h >> 8) as u8, 0x13]);
        }
    }

    pub(crate) fn link_timeouts(&mut self) {
        let now = self.now;
        for i in 0..self.links.len() {
            let Some((_, since)) = self.links[i].pairing else {
                continue;
            };
            if now.saturating_sub(since) < PAIR_MS {
                continue;
            }
            self.links[i].pairing = None;
            let outs = self.links[i]
                .smp
                .as_mut()
                .map(|s| s.abort(reason::TIMEOUT))
                .unwrap_or_default();
            let (h, addr) = (self.links[i].handle, self.links[i].addr);
            self.finish_want(&addr, reason::TIMEOUT);
            self.smp_outs(i, outs);
            self.cmd(op::DISCONNECT, &[h as u8, (h >> 8) as u8, 0x13]);
        }
    }

    /// Whether a device's keys / buttons reach the input sink now.
    pub fn input_from(&self, addr: &Addr) -> bool {
        self.links
            .iter()
            .any(|l| (l.id_addr == *addr || l.addr == *addr) && !l.hid.inputs.is_empty())
    }

    /// Encryption is on: LE pairing finishes, bonded devices start their HID. A bonded
    /// device whose key no longer works is dropped (no pairing behind the person's back).
    pub(crate) fn encrypted(&mut self, i: usize, ok: bool) {
        self.links[i].encrypted = ok;
        let pairing = self.links[i].smp.as_ref().is_some_and(|s| s.active());
        if !ok && !pairing {
            let (h, a) = (self.links[i].handle, self.links[i].id_addr);
            self.finish_want(&a, reason::UNSPECIFIED);
            self.cmd(op::DISCONNECT, &[h as u8, (h >> 8) as u8, 0x05]);
            return;
        }
        if self.links[i].le {
            if pairing {
                let outs = self.links[i]
                    .smp
                    .as_mut()
                    .map(|s| s.encrypted(ok))
                    .unwrap_or_default();
                self.smp_outs(i, outs);
            } else if ok {
                let id = self.links[i].id_addr;
                self.finish_want(&id, 0);
                self.start_hogp(i);
            }
        } else if ok {
            let a = self.links[i].addr;
            self.links[i].pairing = None;
            self.finish_want(&a, 0);
            if self.links[i].central {
                self.open_hid(i); // a device that connected to us opens its channels itself
            }
        }
        self.up.push(Up::Changed);
    }

    fn start_hogp(&mut self, i: usize) {
        if !self.links[i].central || !self.links[i].encrypted || self.links[i].hogp.is_some() {
            return;
        }
        let (c, first) = Hogp::start();
        self.links[i].hogp = Some(c);
        let h = self.links[i].handle;
        self.l2cap(h, l2cap::ATT, &first);
    }

    fn att_pdu(&mut self, i: usize, pdu: &[u8]) {
        let h = self.links[i].handle;
        let encrypted = self.links[i].encrypted;
        let (send, notes) = match self.links[i].hogp.as_mut() {
            Some(c) if encrypted => c.pdu(pdu),
            _ => (att::serve(pdu).into_iter().collect(), vec![]),
        };
        for s in send {
            self.l2cap(h, l2cap::ATT, &s);
        }
        for n in notes {
            match n {
                Note::Ready { keyboard, mouse } => {
                    if keyboard {
                        self.input_added(i, InputKind::Keyboard);
                    }
                    if mouse {
                        self.input_added(i, InputKind::Mouse);
                    }
                }
                Note::Keyboard(r) => self.report(i, InputKind::Keyboard, &r),
                Note::Mouse(r) => self.report(i, InputKind::Mouse, &r),
                Note::Failed(_) => {}
            }
        }
    }

    fn input_id(&self, i: usize, kind: InputKind) -> u32 {
        u32::from(self.links[i].handle) * 2 + u32::from(kind == InputKind::Mouse)
    }

    fn input_added(&mut self, i: usize, kind: InputKind) {
        let id = self.input_id(i, kind);
        if self.links[i].hid.inputs.contains(&id) {
            return;
        }
        self.links[i].hid.inputs.push(id);
        let name = self.name_of(&self.links[i].id_addr);
        self.up.push(Up::InputAdded { id, kind, name });
    }

    fn report(&mut self, i: usize, kind: InputKind, r: &[u8]) {
        let id = self.input_id(i, kind);
        if !self.links[i].hid.inputs.contains(&id) {
            return;
        }
        let events = match kind {
            InputKind::Keyboard => self.links[i].hid.kb.report(r),
            InputKind::Mouse => self.links[i].hid.mouse.report(r),
        };
        if !events.is_empty() {
            self.up.push(Up::Input { id, events });
        }
    }

    pub(crate) fn link_gone(&mut self, i: usize) {
        let l = self.links.remove(i);
        for id in l.hid.inputs {
            self.up.push(Up::InputRemoved { id });
        }
        if l.pairing.is_some() || l.smp.as_ref().is_some_and(|s| s.active()) {
            self.finish_want(&l.addr, reason::UNSPECIFIED);
        }
        self.up.push(Up::Changed);
    }

    fn open_hid(&mut self, i: usize) {
        let id = self.links[i].id_addr;
        let kind = self.kind_of(&id);
        if !matches!(kind, Kind::Keyboard | Kind::Mouse | Kind::Combo)
            || self.links[i].hid.ctrl.is_some()
        {
            return;
        }
        if let Some(pdu) = self.links[i].sig.connect(l2cap::PSM_HID_CONTROL) {
            let h = self.links[i].handle;
            self.links[i].hid.ctrl = self.links[i].sig.channels.last().map(|c| c.local);
            self.l2cap(h, l2cap::SIGNAL, &pdu);
        }
    }

    fn signal(&mut self, i: usize, frame: &[u8]) {
        let bonded = self.bonds.find(&self.links[i].id_addr).is_some() && self.links[i].encrypted;
        let allow = move |psm: u16| {
            bonded && (psm == l2cap::PSM_HID_CONTROL || psm == l2cap::PSM_HID_INTERRUPT)
        };
        let (send, events) = self.links[i].sig.frame(frame, &allow);
        let h = self.links[i].handle;
        for s in send {
            self.l2cap(h, l2cap::SIGNAL, &s);
        }
        for e in events {
            self.sig_event(i, e);
        }
    }

    fn sig_event(&mut self, i: usize, e: SigEvent) {
        let h = self.links[i].handle;
        match e {
            SigEvent::Open(ch) if ch.psm == l2cap::PSM_HID_CONTROL => {
                self.links[i].hid.ctrl = Some(ch.local);
                if self.links[i].hid.intr.is_none() && self.links[i].central {
                    if let Some(pdu) = self.links[i].sig.connect(l2cap::PSM_HID_INTERRUPT) {
                        self.links[i].hid.intr = self.links[i].sig.channels.last().map(|c| c.local);
                        self.l2cap(h, l2cap::SIGNAL, &pdu);
                    }
                }
            }
            SigEvent::Open(ch) if ch.psm == l2cap::PSM_HID_INTERRUPT => {
                self.links[i].hid.intr = Some(ch.local);
                if let Some(ctrl) = self.links[i]
                    .hid
                    .ctrl
                    .and_then(|c| self.links[i].sig.by_local(c))
                    .copied()
                {
                    self.l2cap(h, ctrl.remote, &[hid::SET_PROTOCOL_BOOT]);
                }
                let kind = self.kind_of(&self.links[i].id_addr);
                if kind != Kind::Mouse {
                    self.input_added(i, InputKind::Keyboard);
                }
                if kind != Kind::Keyboard {
                    self.input_added(i, InputKind::Mouse);
                }
            }
            SigEvent::Open(_) => {}
            SigEvent::Closed(cid) => {
                let hs = &mut self.links[i].hid;
                if hs.ctrl == Some(cid) || hs.intr == Some(cid) {
                    hs.ctrl = None;
                    hs.intr = None;
                    for id in std::mem::take(&mut hs.inputs) {
                        self.up.push(Up::InputRemoved { id });
                    }
                }
            }
        }
    }

    fn classic_report(&mut self, i: usize, frame: &[u8]) {
        match hid::classic_input(frame) {
            Some((1, r)) => self.report(i, InputKind::Keyboard, r),
            Some((2, r)) => self.report(i, InputKind::Mouse, r),
            _ => {}
        }
    }
}
