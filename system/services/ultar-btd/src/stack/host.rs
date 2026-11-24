//! The host: one state machine over HCI without I/O. The runner feeds it H4 packets from
//! the controller ([`Host::packet`]), the person's wishes ([`Host::command`]) and the time
//! ([`Host::tick`]); it answers with packets for the controller ([`Host::take_out`]) and
//! news for the service ([`Host::take_up`]).
//!
//! Default deny: nothing pairs unless the person asked for that device ([`Cmd::Pair`]) and
//! confirmed it in the system dialog; incoming connections are accepted only from bonded
//! devices; incoming L2CAP channels only for HID of bonded devices; legacy pairing (PIN,
//! LE legacy) is refused; every step has a time limit.

use super::bonds::{Bond, Bonds, Kind};
use super::hci::{self, op, Addr};
use super::hid::{InputEvent, InputKind};
use super::link::Link;
use super::smp::{Ask, Io};
use std::collections::{BTreeMap, VecDeque};

/// Devices remembered from scans at most.
pub const MAX_DEVICES: usize = 64;
/// A scan ends by itself after this.
pub const SCAN_MS: u64 = 20_000;
/// Connecting gives up after this.
pub const CONNECT_MS: u64 = 15_000;
/// A pairing (with the person's answer, ≤ 55 s in the dialog) ends after this.
pub const PAIR_MS: u64 = 60_000;
/// Queued outgoing ACL packets at most (beyond: the link is dropped).
pub const MAX_ACL_QUEUE: usize = 64;

/// How the host is set up.
#[derive(Clone, Debug)]
pub struct Config {
    /// Our IO capability (Ultar: display + yes/no through the system dialog).
    pub io: Io,
    /// Accept LE pairing requests from centrals (only the test peripheral).
    pub pairable_le: bool,
    /// Advertise (connectable) under this name (only the test peripheral).
    pub advertise: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            io: Io::DisplayYesNo,
            pairable_le: false,
            advertise: None,
        }
    }
}

/// A device as scans and connections showed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// Address (the one to connect to).
    pub addr: Addr,
    /// Name (data).
    pub name: String,
    /// Class of device (BR/EDR).
    pub class: u32,
    /// LE appearance.
    pub appearance: Option<u16>,
    /// Last RSSI.
    pub rssi: Option<i8>,
    /// LE.
    pub le: bool,
    /// Lists the HID service (LE).
    pub hid: bool,
}

impl Device {
    /// The kind, from class of device or appearance.
    pub fn kind(&self) -> Kind {
        if let Some(a) = self.appearance {
            return match a {
                0x03C1 => Kind::Keyboard,
                0x03C2..=0x03C4 => Kind::Mouse,
                0x03C0 => Kind::Combo,
                0x0040..=0x007F => Kind::Phone,
                0x0080..=0x00BF => Kind::Computer,
                0x0940..=0x097F => Kind::Audio,
                _ if self.hid => Kind::Combo,
                _ => Kind::Other,
            };
        }
        match ((self.class >> 8) & 0x1f, (self.class >> 6) & 3) {
            (5, 1) => Kind::Keyboard,
            (5, 2) => Kind::Mouse,
            (5, _) => Kind::Combo,
            (4, _) => Kind::Audio,
            (2, _) => Kind::Phone,
            (1, _) => Kind::Computer,
            _ if self.hid => Kind::Combo,
            _ => Kind::Other,
        }
    }
}

/// The person's wishes.
#[allow(missing_docs)] // the fields are described by their variant
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    /// Start / stop looking for devices.
    Scan(bool),
    /// Pair with a device (then keep it connected).
    Pair(Addr),
    /// Connect a bonded device.
    Connect(Addr),
    /// Disconnect.
    Disconnect(Addr),
    /// Forget a bond (disconnects).
    Forget(Addr),
    /// The answer to [`Up::Ask`].
    Answer { token: u32, accept: bool },
}

/// Why a command was not taken.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// The controller is not (yet) usable.
    NotReady,
    /// Unknown device (not seen, not bonded).
    Unknown,
    /// Another connection attempt is running.
    Busy,
}

/// News for the service.
#[allow(missing_docs)] // the fields are described by their variant
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Up {
    /// The controller is set up (`true`) or unusable (`false`).
    Ready(bool),
    /// Ask the person (system dialog); answer with [`Cmd::Answer`].
    Ask {
        token: u32,
        addr: Addr,
        name: String,
        ask: Ask,
    },
    /// A pairing ended (`reason` 0 = paired).
    Paired { addr: Addr, reason: u8 },
    /// The bond table changed (save it).
    BondsChanged,
    /// The device list or a connection changed.
    Changed,
    /// An input device appeared.
    InputAdded {
        id: u32,
        kind: InputKind,
        name: String,
    },
    /// Input events.
    Input { id: u32, events: Vec<InputEvent> },
    /// An input device left.
    InputRemoved { id: u32 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Want {
    Pair,
    Connect,
}

/// What goes to the controller: commands one at a time, ACL within its buffers.
pub(crate) struct Tx {
    cmds: VecDeque<Vec<u8>>,
    cmd_busy: bool,
    pub(crate) acl_mtu: usize,
    pub(crate) acl_free: u16,
    acl_queue: VecDeque<Vec<u8>>,
    out: Vec<Vec<u8>>,
}

/// The host.
pub struct Host {
    pub(crate) cfg: Config,
    pub(crate) now: u64,
    /// Our address (zero until read).
    pub addr: Addr,
    pub(crate) ready: Option<bool>,
    pub(crate) init_left: usize,
    pub(crate) tx: Tx,
    pub(crate) up: Vec<Up>,
    /// Devices seen.
    pub devices: BTreeMap<Addr, Device>,
    pub(crate) links: Vec<Link>,
    /// Bonds.
    pub bonds: Bonds,
    pub(crate) scanning: Option<u64>,
    pub(crate) want: Option<(Addr, Want, u64)>,
    pub(crate) next_token: u32,
}

impl Host {
    /// A host; the controller is set up from the first [`Host::take_out`] on.
    pub fn new(cfg: Config, bonds: Bonds) -> Self {
        let mut h = Self {
            cfg,
            now: 0,
            addr: Addr::public([0; 6]),
            ready: None,
            init_left: 0,
            tx: Tx {
                cmds: VecDeque::new(),
                cmd_busy: false,
                acl_mtu: 27,
                acl_free: 1,
                acl_queue: VecDeque::new(),
                out: Vec::new(),
            },
            up: Vec::new(),
            devices: BTreeMap::new(),
            links: Vec::new(),
            bonds,
            scanning: None,
            want: None,
            next_token: 0,
        };
        h.init();
        h
    }

    fn init(&mut self) {
        let classic_bonds = self.bonds.list.iter().any(|b| !b.le);
        let mut seq: Vec<(u16, Vec<u8>)> = vec![
            (op::RESET, vec![]),
            (op::READ_BD_ADDR, vec![]),
            (op::READ_BUFFER_SIZE, vec![]),
            (op::LE_READ_BUFFER_SIZE, vec![]),
            (
                op::SET_EVENT_MASK,
                0x3DBF_F807_FFFB_FFFFu64.to_le_bytes().to_vec(),
            ),
            (op::LE_SET_EVENT_MASK, 0x1Fu64.to_le_bytes().to_vec()),
            (op::WRITE_SSP_MODE, vec![1]),
            (op::WRITE_SC_HOST_SUPPORT, vec![1]),
            (op::WRITE_LE_HOST_SUPPORTED, vec![1, 0]),
            (op::WRITE_INQUIRY_MODE, vec![2]),
            // Page scan only while bonded classic devices may reconnect; never discoverable.
            (
                op::WRITE_SCAN_ENABLE,
                vec![if classic_bonds { 2 } else { 0 }],
            ),
        ];
        if let Some(name) = self.cfg.advertise.clone() {
            seq.extend(advertising(&name));
        }
        self.init_left = seq.len();
        for (o, p) in seq {
            self.cmd(o, &p);
        }
    }

    /// Queues a command (one at a time in flight).
    pub(crate) fn cmd(&mut self, opcode: u16, params: &[u8]) {
        self.tx.cmds.push_back(hci::command(opcode, params));
        self.pump();
    }

    pub(crate) fn command_done(&mut self) {
        self.tx.cmd_busy = false;
        self.pump();
    }

    fn pump(&mut self) {
        if !self.tx.cmd_busy {
            if let Some(c) = self.tx.cmds.pop_front() {
                self.tx.cmd_busy = true;
                self.tx.out.push(c);
            }
        }
        while self.tx.acl_free > 0 {
            let Some(p) = self.tx.acl_queue.pop_front() else {
                break;
            };
            self.tx.acl_free -= 1;
            self.tx.out.push(p);
        }
    }

    /// Sends an L2CAP frame on a link.
    pub(crate) fn l2cap(&mut self, handle: u16, cid: u16, payload: &[u8]) {
        for p in super::l2cap::send(handle, cid, payload, self.tx.acl_mtu) {
            if self.tx.acl_queue.len() >= MAX_ACL_QUEUE {
                self.tx.acl_queue.clear();
                self.cmd(op::DISCONNECT, &[handle as u8, (handle >> 8) as u8, 0x13]);
                return;
            }
            self.tx.acl_queue.push_back(p);
        }
        self.pump();
    }

    pub(crate) fn credits(&mut self, n: u16) {
        self.tx.acl_free = self.tx.acl_free.saturating_add(n).min(64);
        self.pump();
    }

    /// Packets for the controller.
    pub fn take_out(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.tx.out)
    }

    /// News for the service.
    pub fn take_up(&mut self) -> Vec<Up> {
        std::mem::take(&mut self.up)
    }

    /// Set up and usable.
    pub fn is_ready(&self) -> bool {
        self.ready == Some(true)
    }

    /// Scanning.
    pub fn is_scanning(&self) -> bool {
        self.scanning.is_some()
    }

    /// The link of a device (by address or bond identity).
    pub fn connected(&self, addr: &Addr) -> bool {
        self.links
            .iter()
            .any(|l| l.addr == *addr || l.id_addr == *addr)
    }

    /// Number of connected devices.
    pub fn connections(&self) -> usize {
        self.links.len()
    }

    pub(crate) fn token(&mut self) -> u32 {
        self.next_token = self.next_token.wrapping_add(1).max(1);
        self.next_token
    }

    pub(crate) fn name_of(&self, addr: &Addr) -> String {
        self.devices
            .get(addr)
            .map(|d| d.name.clone())
            .or_else(|| self.bonds.find(addr).map(|b| b.name.clone()))
            .unwrap_or_default()
    }

    pub(crate) fn kind_of(&self, addr: &Addr) -> Kind {
        match self.devices.get(addr) {
            Some(d) => d.kind(),
            None => self.bonds.find(addr).map(|b| b.kind).unwrap_or(Kind::Other),
        }
    }

    /// A wish of the person.
    pub fn command(&mut self, c: Cmd) -> Result<(), Refusal> {
        if c != Cmd::Scan(false) && !self.is_ready() {
            return Err(Refusal::NotReady);
        }
        match c {
            Cmd::Scan(on) => self.scan(on),
            Cmd::Pair(a) => self.begin(a, Want::Pair)?,
            Cmd::Connect(a) => {
                if self.bonds.find(&a).is_none() {
                    return Err(Refusal::Unknown);
                }
                self.begin(a, Want::Connect)?
            }
            Cmd::Disconnect(a) => self.disconnect(&a)?,
            Cmd::Forget(a) => {
                let _ = self.disconnect(&a);
                if !self.bonds.remove(&a) {
                    return Err(Refusal::Unknown);
                }
                self.up.push(Up::BondsChanged);
                self.up.push(Up::Changed);
            }
            Cmd::Answer { token, accept } => self.answer(token, accept),
        }
        Ok(())
    }

    fn scan(&mut self, on: bool) {
        if on == self.scanning.is_some() {
            return;
        }
        if on {
            self.cmd(op::LE_SET_SCAN_PARAMS, &[1, 0x60, 0, 0x30, 0, 0, 0]);
            self.cmd(op::LE_SET_SCAN_ENABLE, &[1, 0]);
            self.cmd(op::INQUIRY, &[0x33, 0x8b, 0x9e, 8, 0]);
            self.scanning = Some(self.now);
        } else {
            self.cmd(op::LE_SET_SCAN_ENABLE, &[0, 0]);
            self.cmd(op::INQUIRY_CANCEL, &[]);
            self.scanning = None;
        }
        self.up.push(Up::Changed);
    }

    fn begin(&mut self, a: Addr, want: Want) -> Result<(), Refusal> {
        if self.want.is_some() {
            return Err(Refusal::Busy);
        }
        let le = match (self.devices.get(&a), self.bonds.find(&a)) {
            (Some(d), _) => d.le,
            (None, Some(b)) => b.le,
            (None, None) => return Err(Refusal::Unknown),
        };
        if let Some(l) = self
            .links
            .iter()
            .position(|l| l.addr == a || l.id_addr == a)
        {
            // Already connected: pair on the existing link.
            if want == Want::Pair {
                self.want = Some((a, want, self.now));
                self.pair_on(l);
            }
            return Ok(());
        }
        self.scan(false);
        self.want = Some((a, want, self.now));
        if le {
            let mut p = vec![0x60, 0, 0x30, 0, 0, u8::from(a.random)];
            p.extend_from_slice(&a.bytes);
            p.extend_from_slice(&[0, 0x18, 0, 0x28, 0, 0, 0, 0xF4, 0x01, 0, 0, 0, 0]);
            self.cmd(op::LE_CREATE_CONNECTION, &p);
        } else {
            let mut p = a.bytes.to_vec();
            p.extend_from_slice(&[0x18, 0xCC, 0x01, 0, 0, 0, 1]);
            self.cmd(op::CREATE_CONNECTION, &p);
        }
        Ok(())
    }

    fn disconnect(&mut self, a: &Addr) -> Result<(), Refusal> {
        let l = self
            .links
            .iter()
            .find(|l| l.addr == *a || l.id_addr == *a)
            .ok_or(Refusal::Unknown)?;
        let h = l.handle;
        self.cmd(op::DISCONNECT, &[h as u8, (h >> 8) as u8, 0x13]);
        Ok(())
    }

    /// The time (milliseconds since an arbitrary start, never going back).
    pub fn tick(&mut self, now: u64) {
        self.now = self.now.max(now);
        if self.scanning.is_some_and(|t| self.now - t >= SCAN_MS) {
            self.scan(false);
        }
        if let Some((a, w, t)) = self.want {
            let linked = self.connected(&a);
            if self.now - t >= CONNECT_MS && !linked {
                self.want = None;
                if w == Want::Pair || w == Want::Connect {
                    self.cmd(op::LE_CREATE_CONNECTION_CANCEL, &[]);
                    self.up.push(Up::Paired {
                        addr: a,
                        reason: super::smp::reason::TIMEOUT,
                    });
                }
            }
        }
        self.link_timeouts();
    }

    pub(crate) fn finish_want(&mut self, a: &Addr, reason: u8) {
        if let Some((w, kind, _)) = self.want {
            if w == *a {
                self.want = None;
                if kind == Want::Pair {
                    self.up.push(Up::Paired { addr: *a, reason });
                }
            }
        }
    }

    pub(crate) fn wants(&self, a: &Addr) -> Option<Want> {
        self.want.filter(|(w, _, _)| w == a).map(|(_, k, _)| k)
    }

    pub(crate) fn store_bond(&mut self, bond: Bond) {
        self.bonds.put(bond);
        self.up.push(Up::BondsChanged);
    }
}

fn advertising(name: &str) -> Vec<(u16, Vec<u8>)> {
    let n = &name.as_bytes()[..name.len().min(26)];
    let mut data = vec![0u8; 32];
    let rec = [&[2, 0x01, 0x06][..], &[n.len() as u8 + 1, 0x09], n].concat();
    data[0] = rec.len() as u8;
    data[1..1 + rec.len()].copy_from_slice(&rec);
    vec![
        (
            op::LE_SET_ADV_PARAMS,
            vec![0xA0, 0, 0xA0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7, 0],
        ),
        (op::LE_SET_ADV_DATA, data),
        (op::LE_SET_ADV_ENABLE, vec![1]),
    ]
}
