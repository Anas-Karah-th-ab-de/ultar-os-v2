//! HCI events → host state.

use super::bonds::Bond;
use super::hci::{self, op, Addr, Event, Seen, H4_ACL, H4_EVT};
use super::host::{Device, Host, Up, Want, MAX_DEVICES};
use super::link::Link;
use super::smp::{reason, Ask};

/// Link key types of Secure Simple Pairing (P-192 / P-256, (un)authenticated); legacy
/// keys (0–2), debug keys (3) and changed combination keys (6) are never stored.
const SSP_KEYS: [u8; 4] = [4, 5, 7, 8];

impl Host {
    /// One H4 packet from the controller. Malformed packets are dropped.
    pub fn packet(&mut self, p: &[u8]) {
        match p.split_first() {
            Some((&H4_EVT, rest)) => {
                if let Ok(e) = hci::event(rest) {
                    self.event(e);
                }
            }
            Some((&H4_ACL, rest)) => {
                if let Ok((h, first, data)) = hci::acl_in(rest) {
                    self.acl(h, first, data);
                }
            }
            _ => {}
        }
    }

    fn event(&mut self, e: Event) {
        match e {
            Event::CommandComplete {
                opcode,
                status,
                data,
            } => self.complete(opcode, status, &data),
            Event::CommandStatus { opcode, status } => self.status(opcode, status),
            Event::Seen(list) => list.into_iter().for_each(|s| self.seen(s)),
            Event::RemoteName { status, addr, name } => {
                let name = hci::clean_name(&name);
                if let Some(d) = self.devices.get_mut(&addr).filter(|_| status == 0) {
                    if !name.is_empty() && d.name != name {
                        d.name = name;
                        self.up.push(Up::Changed);
                    }
                }
                self.name_done(Some(addr));
            }
            Event::InquiryComplete => self.discovery_ended(true, false),
            Event::ConnectionComplete {
                status,
                handle,
                addr,
                acl,
            } => self.classic_connected(status, handle, addr, acl),
            Event::ConnectionRequest { addr, acl, .. } => self.connection_request(addr, acl),
            Event::LeConnected {
                status,
                handle,
                central,
                addr,
            } => self.le_connected(status, handle, central, addr),
            Event::Disconnected { handle, .. } => {
                if let Some(i) = self.link(handle) {
                    self.link_gone(i);
                }
            }
            Event::EncryptionChange { status, handle, on } => {
                if let Some(i) = self.link(handle) {
                    self.encrypted(i, status == 0 && on);
                }
            }
            Event::CompletedPackets(list) => {
                let n = list.iter().fold(0u16, |a, (_, c)| a.saturating_add(*c));
                self.credits(n);
            }
            Event::LeLtkRequest { handle, rand, ediv } => self.ltk_request(handle, rand, ediv),
            other => self.ssp_event(other),
        }
    }

    fn complete(&mut self, opcode: u16, status: u8, data: &[u8]) {
        if opcode == 0 {
            return; // controller's start credit
        }
        if status != 0 {
            self.up.push(Up::Failed { opcode, status });
        }
        let mut r = hci::Rd(data);
        match (opcode, status) {
            (op::READ_LOCAL_VERSION, 0) => {
                if let (Ok(v), Ok(_), Ok(_), Ok(m)) = (r.u8(), r.u16(), r.u8(), r.u16()) {
                    self.version = Some((v, m));
                }
            }
            (op::LE_SET_SCAN_ENABLE, s) if s != 0 && self.discovery.1 => {
                self.discovery_ended(false, true)
            }
            (op::INQUIRY_CANCEL, _) => {}
            (op::READ_BD_ADDR, 0) => {
                if let Ok(a) = r.addr() {
                    self.addr = a;
                }
            }
            (op::READ_BUFFER_SIZE, 0) => {
                if let (Ok(len), Ok(_), Ok(n)) = (r.u16(), r.u8(), r.u16()) {
                    self.tx.acl_mtu = usize::from(len).clamp(23, 1021);
                    self.tx.acl_free = n.clamp(1, 64);
                }
            }
            (op::LE_READ_BUFFER_SIZE, 0) => {
                if let (Ok(len), Ok(n)) = (r.u16(), r.u8()) {
                    if len > 0 && n > 0 {
                        self.tx.acl_mtu = self.tx.acl_mtu.min(usize::from(len).max(23));
                        self.tx.acl_free = self.tx.acl_free.min(u16::from(n));
                    }
                }
            }
            (op::RESET | op::READ_BD_ADDR, s) if s != 0 && self.ready.is_none() => {
                self.ready = Some(false);
                self.up.push(Up::Ready(false));
            }
            (op::LE_CREATE_CONNECTION_CANCEL, _) => {}
            _ => {}
        }
        self.init_step(opcode);
        self.command_done();
    }

    fn init_step(&mut self, opcode: u16) {
        if self.ready.is_some() || self.init_left == 0 || opcode == op::INQUIRY {
            return;
        }
        self.init_left -= 1;
        if self.init_left == 0 {
            self.ready = Some(true);
            self.up.push(Up::Ready(true));
        }
    }

    fn status(&mut self, opcode: u16, status: u8) {
        if status != 0 {
            self.up.push(Up::Failed { opcode, status });
            match opcode {
                op::INQUIRY => self.discovery_ended(true, true),
                op::REMOTE_NAME_REQUEST => self.name_done(None),
                op::LE_CREATE_CONNECTION | op::CREATE_CONNECTION => {
                    if let Some((a, _, _)) = self.want {
                        self.finish_want(&a, reason::UNSPECIFIED);
                        self.want = None;
                    }
                }
                _ => {}
            }
        }
        self.init_step(opcode);
        self.command_done();
    }

    fn seen(&mut self, s: Seen) {
        let adv = hci::adv(&s.data);
        let known = self.devices.contains_key(&s.addr);
        if !known && self.devices.len() >= MAX_DEVICES {
            return;
        }
        if s.le && !s.connectable && !known {
            return;
        }
        let d = self.devices.entry(s.addr).or_insert_with(|| Device {
            addr: s.addr,
            name: String::new(),
            class: 0,
            appearance: None,
            rssi: None,
            le: s.le,
            hid: false,
        });
        d.rssi = s.rssi.or(d.rssi);
        if !s.le {
            d.class = s.class;
        }
        // a shortened name only until the complete one is known (advertisement and scan
        // response may carry either)
        if let Some(n) = adv.name.filter(|n| !n.is_empty()) {
            if adv.name_complete || d.name.is_empty() {
                d.name = n;
            }
        }
        d.appearance = adv.appearance.or(d.appearance);
        d.hid |= adv.hid;
        let ask_name = !s.le && d.name.is_empty();
        if ask_name {
            self.want_name(s.addr, s.page);
        }
        self.up.push(Up::Changed);
    }

    fn classic_connected(&mut self, status: u8, handle: u16, addr: Addr, acl: bool) {
        if !acl {
            return;
        }
        let want = self.wants(&addr);
        if status != 0 {
            self.finish_want(&addr, reason::UNSPECIFIED);
            self.want = self.want.filter(|w| w.0 != addr);
            return;
        }
        self.links
            .push(Link::new(handle, addr, addr, false, want.is_some()));
        let i = self.links.len() - 1;
        match want {
            Some(Want::Pair) => self.pair_on(i),
            Some(Want::Connect) => self.cmd(op::AUTH_REQUESTED, &handle.to_le_bytes()),
            None => {}
        }
        self.up.push(Up::Changed);
    }

    fn connection_request(&mut self, addr: Addr, acl: bool) {
        let bonded = self.bonds.find(&addr).is_some_and(|b| !b.le);
        if acl && bonded {
            let mut p = addr.bytes.to_vec();
            p.push(0x01); // stay peripheral
            self.cmd(op::ACCEPT_CONNECTION, &p);
        } else {
            let mut p = addr.bytes.to_vec();
            p.push(0x0F); // unacceptable device address
            self.cmd(op::REJECT_CONNECTION, &p);
        }
    }

    fn le_connected(&mut self, status: u8, handle: u16, central: bool, addr: Addr) {
        let id_addr = self.bonds.find(&addr).map(|b| b.addr).unwrap_or(addr);
        let want = self.wants(&addr).or_else(|| self.wants(&id_addr));
        if status != 0 {
            if central {
                self.finish_want(&addr, reason::UNSPECIFIED);
                self.want = None;
            }
            return;
        }
        if central && want.is_none() {
            // A connection nobody asked for any more (cancelled, timed out): drop it.
            self.cmd(op::DISCONNECT, &[handle as u8, (handle >> 8) as u8, 0x13]);
            return;
        }
        self.links
            .push(Link::new(handle, addr, id_addr, true, central));
        let i = self.links.len() - 1;
        match want {
            Some(Want::Pair) => self.pair_on(i),
            Some(Want::Connect) => {
                if let Some(key) = self.bonds.find(&id_addr).map(|b| b.key) {
                    self.start_encryption(handle, &key);
                }
            }
            None => {}
        }
        self.up.push(Up::Changed);
    }

    fn ltk_request(&mut self, handle: u16, rand: [u8; 8], ediv: u16) {
        let Some(i) = self.link(handle) else { return };
        let during = self.links[i].smp.as_ref().and_then(|s| s.ltk());
        let bonded = (rand == [0; 8] && ediv == 0)
            .then(|| {
                self.bonds
                    .find(&self.links[i].id_addr)
                    .filter(|b| b.le)
                    .map(|b| b.key)
            })
            .flatten();
        let mut p = handle.to_le_bytes().to_vec();
        match during.map(|k| *k).or(bonded) {
            Some(key) => {
                p.extend_from_slice(&key);
                self.cmd(op::LE_LTK_REPLY, &p);
            }
            None => self.cmd(op::LE_LTK_NEG_REPLY, &p),
        }
    }

    fn classic_link(&self, addr: &Addr) -> Option<usize> {
        self.links.iter().position(|l| !l.le && l.addr == *addr)
    }

    fn ssp_event(&mut self, e: Event) {
        match e {
            Event::PinCodeRequest(a) => self.cmd(op::PIN_CODE_NEG_REPLY, &a.bytes),
            Event::LinkKeyRequest(a) => {
                let fresh = self.wants(&a) == Some(Want::Pair);
                match self
                    .bonds
                    .find(&a)
                    .filter(|b| !b.le && !fresh)
                    .map(|b| b.key)
                {
                    Some(key) => self.cmd(op::LINK_KEY_REPLY, &[&a.bytes[..], &key].concat()),
                    None => self.cmd(op::LINK_KEY_NEG_REPLY, &a.bytes),
                }
            }
            Event::IoCapRequest(a) => self.io_cap_request(a),
            Event::IoCapResponse { addr, io, .. } => {
                if let Some(i) = self.classic_link(&addr) {
                    self.links[i].peer_io = io;
                }
            }
            Event::UserConfirm { addr, value } => match self.classic_link(&addr) {
                Some(i) if self.links[i].pairing.is_some() => {
                    let jw = self.links[i].peer_io == 3 || self.links[i].peer_io == 0;
                    self.ask(
                        i,
                        if jw {
                            Ask::Consent
                        } else {
                            Ask::Compare(value % 1_000_000)
                        },
                    );
                }
                _ => self.cmd(op::USER_CONFIRM_NEG_REPLY, &addr.bytes),
            },
            Event::UserPasskeyRequest(a) => self.cmd(op::USER_PASSKEY_NEG_REPLY, &a.bytes),
            Event::PasskeyNotification { addr, passkey } => {
                if let Some(i) = self
                    .classic_link(&addr)
                    .filter(|&i| self.links[i].pairing.is_some())
                {
                    self.links[i].peer_io = 0xfe;
                    self.ask(i, Ask::Show(passkey % 1_000_000));
                }
            }
            Event::SspComplete { status, addr } if status != 0 => {
                if let Some(i) = self.classic_link(&addr) {
                    self.links[i].pairing = None;
                }
                self.finish_want(&addr, reason::UNSPECIFIED);
            }
            Event::LinkKeyNotification { addr, key, kind } => self.link_key(addr, key, kind),
            Event::AuthComplete { status, handle } => self.auth_complete(status, handle),
            _ => {}
        }
    }

    fn auth_complete(&mut self, status: u8, handle: u16) {
        if status == 0 {
            self.cmd(
                op::SET_CONN_ENCRYPTION,
                &[handle as u8, (handle >> 8) as u8, 1],
            );
        } else if let Some(i) = self.link(handle) {
            let a = self.links[i].addr;
            self.links[i].pairing = None;
            self.finish_want(&a, reason::UNSPECIFIED);
            self.cmd(op::DISCONNECT, &[handle as u8, (handle >> 8) as u8, 0x05]);
        }
    }

    fn io_cap_request(&mut self, a: Addr) {
        let asked = self
            .classic_link(&a)
            .is_some_and(|i| self.links[i].pairing.is_some());
        if asked {
            self.cmd(
                op::IO_CAPABILITY_REPLY,
                &[&a.bytes[..], &[self.cfg.io as u8, 0, 0x05]].concat(),
            );
        } else {
            self.cmd(
                op::IO_CAPABILITY_NEG_REPLY,
                &[&a.bytes[..], &[0x18]].concat(),
            );
        }
    }

    fn link_key(&mut self, addr: Addr, key: [u8; 16], kind: u8) {
        let asked = self
            .classic_link(&addr)
            .is_some_and(|i| self.links[i].pairing.is_some());
        if !SSP_KEYS.contains(&kind) || !asked {
            return;
        }
        let bond = Bond {
            addr,
            le: false,
            key,
            key_type: kind,
            authenticated: kind == 5 || kind == 8,
            irk: None,
            name: self.name_of(&addr),
            kind: self.kind_of(&addr),
        };
        self.store_bond(bond);
    }
}
