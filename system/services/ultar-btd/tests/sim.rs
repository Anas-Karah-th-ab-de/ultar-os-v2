//! End to end without hardware: two hosts over two **simulated LE controllers** joined by
//! a simulated air. The kernel's virtual controller (`/dev/vhci`, `btvirt`) is not
//! available here (no Bluetooth in the build machine's kernel), so the controllers are
//! simulated at the HCI boundary: commands answered as Core Vol 4 Part E says, ACL
//! forwarded, encryption granted only when both sides hold the same LTK – which is what
//! proves the two SMP state machines derived the same key with real crypto.

use ultar_btd::stack::hci::{op, Addr};
use ultar_btd::stack::host::{Cmd, Config, Host, Up};
use ultar_btd::stack::smp::{Ask, Io};

struct Ctl {
    addr: [u8; 6],
    adv: Option<Vec<u8>>,
    scanning: bool,
    connected: bool,
    pending_ltk: Option<Vec<u8>>,
    inbox: Vec<Vec<u8>>,
}

fn evt(code: u8, params: &[u8]) -> Vec<u8> {
    let mut v = vec![0x04, code, params.len() as u8];
    v.extend_from_slice(params);
    v
}

fn complete(opcode: u16, ret: &[u8]) -> Vec<u8> {
    let mut p = vec![1];
    p.extend_from_slice(&opcode.to_le_bytes());
    p.extend_from_slice(ret);
    evt(0x0E, &p)
}

fn status(opcode: u16, st: u8) -> Vec<u8> {
    let mut p = vec![st, 1];
    p.extend_from_slice(&opcode.to_le_bytes());
    evt(0x0F, &p)
}

fn le_connected(central: bool, peer: [u8; 6]) -> Vec<u8> {
    let mut p = vec![0x01, 0, 0x40, 0, u8::from(!central), 0];
    p.extend_from_slice(&peer);
    p.extend_from_slice(&[0x18, 0, 0, 0, 0xF4, 1, 0]);
    evt(0x3E, &p)
}

fn enc(st: u8, on: bool) -> Vec<u8> {
    evt(0x08, &[st, 0x40, 0, u8::from(on)])
}

struct Air {
    c: [Ctl; 2],
    h: [Host; 2],
    ups: [Vec<Up>; 2],
}

impl Air {
    fn new(cfg_b: Config) -> Self {
        let ctl = |last| Ctl {
            addr: [last, 0xaa, 0xbb, 0xcc, 0xdd, 0xee],
            adv: None,
            scanning: false,
            connected: false,
            pending_ltk: None,
            inbox: vec![],
        };
        let mut air = Self {
            c: [ctl(1), ctl(2)],
            h: [
                Host::new(Config::default(), Default::default()),
                Host::new(cfg_b, Default::default()),
            ],
            ups: [vec![], vec![]],
        };
        air.run();
        assert!(air.h[0].is_ready() && air.h[1].is_ready());
        air
    }

    fn addr(&self, i: usize) -> Addr {
        Addr::public(self.c[i].addr)
    }

    /// Moves packets until nothing happens any more.
    fn run(&mut self) {
        for _ in 0..10_000 {
            let mut busy = false;
            for i in 0..2 {
                for p in self.h[i].take_out() {
                    busy = true;
                    self.controller(i, &p);
                }
                for p in std::mem::take(&mut self.c[i].inbox) {
                    busy = true;
                    self.h[i].packet(&p);
                }
                self.ups[i].extend(self.h[i].take_up());
            }
            if !busy {
                return;
            }
        }
        panic!("no rest");
    }

    fn controller(&mut self, i: usize, p: &[u8]) {
        let j = 1 - i;
        if p[0] == 0x02 {
            // ACL: forward, give the credit back.
            self.c[j].inbox.push(p.to_vec());
            self.c[i].inbox.push(evt(0x13, &[1, 0x40, 0, 1, 0]));
            return;
        }
        let opcode = u16::from_le_bytes([p[1], p[2]]);
        let params = p[4..].to_vec();
        let reply = match opcode {
            op::READ_BD_ADDR => complete(opcode, &[&[0][..], &self.c[i].addr].concat()),
            op::READ_BUFFER_SIZE => complete(opcode, &[0, 27, 0, 0, 8, 0, 0, 0]),
            op::LE_READ_BUFFER_SIZE => complete(opcode, &[0, 27, 0, 8]),
            op::LE_SET_ADV_DATA => {
                let len = usize::from(params[0]);
                self.c[i].adv = Some(params[1..1 + len].to_vec());
                complete(opcode, &[0])
            }
            op::LE_SET_SCAN_ENABLE => return self.scan(i, opcode, &params),
            op::LE_CREATE_CONNECTION => return self.connect(i, opcode, &params),
            op::LE_START_ENCRYPTION => {
                self.c[i].pending_ltk = Some(params[12..28].to_vec());
                self.c[i].inbox.push(status(opcode, 0));
                self.c[j]
                    .inbox
                    .push(evt(0x3E, &[&[0x05, 0x40, 0][..], &params[2..12]].concat()));
                return;
            }
            op::LE_LTK_REPLY | op::LE_LTK_NEG_REPLY => {
                let theirs = self.c[j].pending_ltk.take();
                let same = opcode == op::LE_LTK_REPLY && theirs.as_deref() == Some(&params[2..18]);
                self.c[i].inbox.push(complete(opcode, &[0, 0x40, 0]));
                let e = if same { enc(0, true) } else { enc(0x06, false) };
                self.c[i].inbox.push(e.clone());
                self.c[j].inbox.push(e);
                return;
            }
            op::DISCONNECT => {
                self.c[i].inbox.push(status(opcode, 0));
                for k in [i, j] {
                    if self.c[k].connected {
                        self.c[k].connected = false;
                        self.c[k].inbox.push(evt(0x05, &[0, 0x40, 0, params[2]]));
                    }
                }
                return;
            }
            op::INQUIRY | op::CREATE_CONNECTION => status(opcode, 0),
            _ => complete(opcode, &[0]),
        };
        self.c[i].inbox.push(reply);
    }

    fn scan(&mut self, i: usize, opcode: u16, params: &[u8]) {
        let j = 1 - i;
        self.c[i].scanning = params[0] == 1;
        if let (true, Some(adv)) = (self.c[i].scanning, self.c[j].adv.clone()) {
            let mut r = vec![0x02, 1, 0x00, 0x00];
            r.extend_from_slice(&self.c[j].addr);
            r.push(adv.len() as u8);
            r.extend_from_slice(&adv);
            r.push(0xCE); // -50 dBm
            self.c[i].inbox.push(complete(opcode, &[0]));
            self.c[i].inbox.push(evt(0x3E, &r));
            return;
        }
        self.c[i].inbox.push(complete(opcode, &[0]));
    }

    fn connect(&mut self, i: usize, opcode: u16, params: &[u8]) {
        let j = 1 - i;
        let target = &params[6..12];
        self.c[i].inbox.push(status(opcode, 0));
        if self.c[j].adv.is_some() && target == self.c[j].addr && !self.c[i].connected {
            self.c[i].connected = true;
            self.c[j].connected = true;
            let (a, b) = (self.c[j].addr, self.c[i].addr);
            self.c[i].inbox.push(le_connected(true, a));
            self.c[j].inbox.push(le_connected(false, b));
        }
    }

    fn asks(&mut self, i: usize) -> Vec<(u32, Ask)> {
        let mut out = vec![];
        self.ups[i].retain(|u| match u {
            Up::Ask { token, ask, .. } => {
                out.push((*token, *ask));
                false
            }
            _ => true,
        });
        out
    }

    fn paired(&self, i: usize) -> Option<u8> {
        self.ups[i].iter().rev().find_map(|u| match u {
            Up::Paired { reason, .. } => Some(*reason),
            _ => None,
        })
    }
}

fn peripheral() -> Config {
    Config {
        io: Io::DisplayYesNo,
        pairable_le: true,
        advertise: Some("Ultar Test Keyboard".into()),
    }
}

fn pair(air: &mut Air, a_says: bool, b_says: bool) {
    let b = air.addr(1);
    air.h[0].command(Cmd::Scan(true)).unwrap();
    air.run();
    assert_eq!(
        air.h[0].devices.get(&b).map(|d| d.name.as_str()),
        Some("Ultar Test Keyboard")
    );
    assert_eq!(air.h[0].devices.get(&b).and_then(|d| d.rssi), Some(-50));
    air.h[0].command(Cmd::Pair(b)).unwrap();
    air.run();
    let (qa, qb) = (air.asks(0), air.asks(1));
    let (Some(&(ta, Ask::Compare(na))), Some(&(tb, Ask::Compare(nb)))) = (qa.first(), qb.first())
    else {
        panic!("numeric comparison on both: {qa:?} {qb:?}");
    };
    assert_eq!(na, nb, "both screens show the same number");
    air.h[1]
        .command(Cmd::Answer {
            token: tb,
            accept: b_says,
        })
        .unwrap();
    air.run();
    air.h[0]
        .command(Cmd::Answer {
            token: ta,
            accept: a_says,
        })
        .unwrap();
    air.run();
}

#[test]
fn two_hosts_pair_with_secure_connections_and_reconnect_with_the_bond() {
    let mut air = Air::new(peripheral());
    let (a, b) = (air.addr(0), air.addr(1));
    pair(&mut air, true, true);
    assert_eq!(air.paired(0), Some(0));
    assert!(air.h[0].connected(&b));
    let (ka, kb) = (
        air.h[0].bonds.find(&b).unwrap(),
        air.h[1].bonds.find(&a).unwrap(),
    );
    assert_eq!(ka.key, kb.key, "same LTK on both sides");
    assert!(ka.authenticated);
    assert!(air.ups[0].contains(&Up::BondsChanged));
    // Disconnect, connect again: encryption from the bond, no question.
    air.h[0].command(Cmd::Disconnect(b)).unwrap();
    air.run();
    assert!(!air.h[0].connected(&b));
    air.ups = [vec![], vec![]];
    air.h[0].command(Cmd::Connect(b)).unwrap();
    air.run();
    assert!(air.h[0].connected(&b));
    assert!(air.asks(0).is_empty() && air.asks(1).is_empty());
    assert!(air.ups[0]
        .iter()
        .all(|u| !matches!(u, Up::Paired { reason, .. } if *reason != 0)));
    // Forget: the bond is gone and the link closed; connecting is refused.
    air.h[0].command(Cmd::Forget(b)).unwrap();
    air.run();
    assert!(air.h[0].bonds.find(&b).is_none() && !air.h[0].connected(&b));
    assert!(air.h[0].command(Cmd::Connect(b)).is_err());
}

#[test]
fn a_no_on_either_screen_leaves_no_bond() {
    for (sa, sb) in [(true, false), (false, true)] {
        let mut air = Air::new(peripheral());
        let (a, b) = (air.addr(0), air.addr(1));
        pair(&mut air, sa, sb);
        assert!(matches!(air.paired(0), Some(r) if r != 0), "{sa} {sb}");
        assert!(air.h[0].bonds.find(&b).is_none() && air.h[1].bonds.find(&a).is_none());
        assert!(!air.h[0].connected(&b), "the link is dropped");
    }
}

#[test]
fn a_device_that_is_not_pairable_or_a_stale_key_gets_nothing() {
    let mut cfg = peripheral();
    cfg.pairable_le = false;
    let mut air = Air::new(cfg);
    let b = air.addr(1);
    air.h[0].command(Cmd::Scan(true)).unwrap();
    air.run();
    air.h[0].command(Cmd::Pair(b)).unwrap();
    air.run();
    assert!(air.asks(0).is_empty());
    assert!(matches!(air.paired(0), Some(r) if r != 0));
    assert!(air.h[0].bonds.find(&b).is_none());
    // A bond the other side no longer has: encryption fails, nothing is delivered.
    let mut air = Air::new(peripheral());
    pair(&mut air, true, true);
    let a = air.addr(0);
    air.h[0].command(Cmd::Disconnect(b)).unwrap();
    air.run();
    air.h[1].bonds.remove(&a);
    air.ups = [vec![], vec![]];
    air.h[0].command(Cmd::Connect(b)).unwrap();
    air.run();
    assert!(air.ups[0]
        .iter()
        .all(|u| !matches!(u, Up::InputAdded { .. })));
    assert!(
        air.asks(1).is_empty(),
        "no pairing started behind the person's back"
    );
}

#[test]
fn commands_before_the_controller_is_ready_and_unknown_devices_are_refused() {
    let mut h = Host::new(Config::default(), Default::default());
    assert!(h.command(Cmd::Scan(true)).is_err());
    let mut air = Air::new(peripheral());
    let stranger = Addr::public([9; 6]);
    assert!(air.h[0].command(Cmd::Pair(stranger)).is_err());
    assert!(
        air.h[0].command(Cmd::Connect(air.addr(1))).is_err(),
        "not bonded"
    );
    let _ = &mut h;
}
