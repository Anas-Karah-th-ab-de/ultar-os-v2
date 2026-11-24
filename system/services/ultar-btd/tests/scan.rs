//! Looking for devices as an HCI trace (Core Vol 4 Part E 7.7 layouts): a classic inquiry
//! (inquiry results in both RSSI layouts, extended inquiry results with names, remote name
//! requests after the inquiry) and an LE scan (active and passive, advertising reports and
//! scan responses with names), devices kept once per address with their latest signal, and
//! a scan the controller refuses ending at once.

use ultar_btd::stack::hci::{op, Addr};
use ultar_btd::stack::host::{Cmd, Config, Host, Up};

fn evt(code: u8, params: &[u8]) -> Vec<u8> {
    [&[0x04, code, params.len() as u8][..], params].concat()
}

/// The controller's answers: `how` gives the status for a command (`None`: no answer at
/// all); returns what was sent.
fn drain_how(h: &mut Host, how: impl Fn(u16) -> Option<u8>) -> Vec<(u16, Vec<u8>)> {
    let mut sent = Vec::new();
    for _ in 0..100 {
        let out = h.take_out();
        if out.is_empty() {
            break;
        }
        for p in out {
            let opcode = u16::from_le_bytes([p[1], p[2]]);
            sent.push((opcode, p[4..].to_vec()));
            let Some(st) = how(opcode) else { continue };
            if [op::INQUIRY, op::REMOTE_NAME_REQUEST].contains(&opcode) {
                h.packet(&evt(0x0F, &[st, 1, p[1], p[2]]));
            } else if opcode == op::READ_BD_ADDR {
                h.packet(&evt(0x0E, &[1, p[1], p[2], st, 1, 2, 3, 4, 5, 6]));
            } else if opcode == op::READ_LOCAL_VERSION {
                h.packet(&evt(0x0E, &[1, p[1], p[2], st, 12, 0, 0, 12, 2, 0, 0, 0]));
            } else {
                h.packet(&evt(0x0E, &[1, p[1], p[2], st]));
            }
        }
    }
    sent
}

/// Every command succeeds unless `refuse` names it (then with status 0x01, unknown
/// command).
fn drain_with(h: &mut Host, refuse: &[u16]) -> Vec<(u16, Vec<u8>)> {
    drain_how(h, |o| Some(u8::from(refuse.contains(&o))))
}

fn drain(h: &mut Host) -> Vec<(u16, Vec<u8>)> {
    drain_with(h, &[])
}

fn ops(sent: &[(u16, Vec<u8>)]) -> Vec<u16> {
    sent.iter().map(|s| s.0).collect()
}

fn ready(cfg: Config) -> Host {
    let mut h = Host::new(cfg, Default::default());
    drain(&mut h);
    assert!(h.is_ready());
    assert_eq!(
        h.version,
        Some((12, 2)),
        "Read Local Version: HCI 5.3, Intel"
    );
    h
}

const A: [u8; 6] = [0xA1, 0, 0, 0, 0, 0x10];
const B: [u8; 6] = [0xB1, 0, 0, 0, 0, 0x10];
const C: [u8; 6] = [0xC1, 0, 0, 0, 0, 0x10];
const D: [u8; 6] = [0xD1, 0, 0, 0, 0, 0xC0];
const E: [u8; 6] = [0xE1, 0, 0, 0, 0, 0xC0];
const F: [u8; 6] = [0xF1, 0, 0, 0, 0, 0xC0];

/// Inquiry Result with RSSI (0x22), one response per device, packed one after another
/// (`pscan`: the 15-octet layout with the page scan mode octet).
fn inquiry_rssi(devs: &[([u8; 6], u32, i8)], pscan: bool) -> Vec<u8> {
    let mut p = vec![devs.len() as u8];
    for (a, class, rssi) in devs {
        p.extend_from_slice(a);
        p.push(1); // page scan repetition mode R1
        p.extend_from_slice(if pscan { &[0, 0][..] } else { &[0][..] });
        p.extend_from_slice(&class.to_le_bytes()[..3]);
        p.extend_from_slice(&[0x34, 0x12]); // clock offset
        p.push(*rssi as u8);
    }
    evt(0x22, &p)
}

/// Extended Inquiry Result (0x2F) with a complete name.
fn eir(a: [u8; 6], class: u32, rssi: i8, name: &str) -> Vec<u8> {
    let mut p = vec![1];
    p.extend_from_slice(&a);
    p.extend_from_slice(&[1, 0]);
    p.extend_from_slice(&class.to_le_bytes()[..3]);
    p.extend_from_slice(&[0x34, 0x12, rssi as u8]);
    p.push(name.len() as u8 + 1);
    p.push(0x09);
    p.extend_from_slice(name.as_bytes());
    p.resize(255, 0);
    evt(0x2F, &p)
}

/// LE Advertising Report (one report): `kind` 0 ADV_IND, 3 ADV_NONCONN_IND, 4 SCAN_RSP;
/// `name`: (complete, text).
fn adv(kind: u8, a: [u8; 6], rssi: i8, name: Option<(bool, &str)>) -> Vec<u8> {
    let mut data = vec![2, 0x01, 0x06];
    if let Some((complete, n)) = name {
        data.push(n.len() as u8 + 1);
        data.push(if complete { 0x09 } else { 0x08 });
        data.extend_from_slice(n.as_bytes());
    }
    let mut p = vec![0x02, 1, kind, 1];
    p.extend_from_slice(&a);
    p.push(data.len() as u8);
    p.extend_from_slice(&data);
    p.push(rssi as u8);
    evt(0x3E, &p)
}

fn remote_name(status: u8, a: [u8; 6], name: &str) -> Vec<u8> {
    let mut p = vec![status];
    p.extend_from_slice(&a);
    p.extend_from_slice(name.as_bytes());
    p.resize(255, 0);
    evt(0x07, &p)
}

fn name_of(h: &Host, a: Addr) -> &str {
    &h.devices[&a].name
}

#[test]
fn inquiry_and_le_scan_find_devices_with_names_and_their_signal() {
    let mut h = ready(Config::default());
    h.command(Cmd::Scan(true)).unwrap();
    let sent = drain(&mut h);
    let params = |o| sent.iter().find(|s| s.0 == o).unwrap().1.clone();
    assert_eq!(params(op::LE_SET_SCAN_PARAMS)[0], 1, "active scan");
    assert_eq!(
        params(op::LE_SET_SCAN_ENABLE),
        [1, 0],
        "on, duplicates reported"
    );
    assert_eq!(
        params(op::INQUIRY),
        [0x33, 0x8b, 0x9e, 8, 0],
        "GIAC, 10.24 s"
    );
    assert!(h.is_scanning());

    // classic: two nameless devices in one result (14-octet layout), one in the 15-octet
    // layout, one with its name in the extended inquiry result
    let (a, b, c) = (Addr::public(A), Addr::public(B), Addr::public(C));
    h.packet(&inquiry_rssi(
        &[(A, 0x5a020c, -70), (B, 0x240404, -80)],
        false,
    ));
    h.packet(&inquiry_rssi(&[(B, 0x240404, -75)], true));
    h.packet(&eir(C, 0x002540, -60, "K380"));
    h.packet(&eir(C, 0x002540, -50, "K380"));
    assert_eq!(h.devices[&b].rssi, Some(-75), "the 15-octet layout read");
    assert_eq!(h.devices[&c].rssi, Some(-50), "the latest signal");
    assert_eq!(name_of(&h, c), "K380");
    assert!(
        !ops(&drain(&mut h)).contains(&op::REMOTE_NAME_REQUEST),
        "no name request while the inquiry runs"
    );

    // LE: a named mouse seen twice, a pen naming itself only in its scan response (the
    // shortened name of a later advertisement does not replace it), a beacon not listed
    let (d, e, f) = (
        Addr {
            bytes: D,
            random: true,
        },
        Addr {
            bytes: E,
            random: true,
        },
        Addr {
            bytes: F,
            random: true,
        },
    );
    h.packet(&adv(0, D, -71, Some((true, "Mouse"))));
    h.packet(&adv(0, D, -65, Some((true, "Mouse"))));
    h.packet(&adv(0, E, -80, None));
    assert_eq!(name_of(&h, e), "", "no name yet");
    h.packet(&adv(4, E, -79, Some((true, "Pen"))));
    h.packet(&adv(0, E, -78, Some((false, "P"))));
    h.packet(&adv(3, F, -40, Some((true, "Beacon"))));
    assert_eq!(name_of(&h, d), "Mouse");
    assert_eq!(h.devices[&d].rssi, Some(-65));
    assert_eq!(name_of(&h, e), "Pen");
    assert_eq!(h.devices[&e].rssi, Some(-78));
    assert!(!h.devices.contains_key(&f), "not connectable");
    assert_eq!(h.devices.len(), 5, "one entry per address");
    assert!(h.take_up().contains(&Up::Changed));

    // the inquiry ends: the nameless ones are asked for their names, one at a time
    h.packet(&evt(0x01, &[0]));
    let sent = drain(&mut h);
    let rnr: Vec<_> = sent
        .iter()
        .filter(|s| s.0 == op::REMOTE_NAME_REQUEST)
        .collect();
    assert_eq!(rnr.len(), 1, "one name request in flight");
    assert_eq!(
        rnr[0].1,
        [&A[..], &[1, 0, 0x34, 0x92]].concat(),
        "R1, clock offset"
    );
    h.packet(&remote_name(0, A, "Phone A"));
    let sent = drain(&mut h);
    assert_eq!(name_of(&h, a), "Phone A");
    assert_eq!(ops(&sent), [op::REMOTE_NAME_REQUEST], "then the next");
    h.packet(&remote_name(0x04, B, ""));
    assert!(
        drain(&mut h).is_empty(),
        "page timeout: nothing more to ask"
    );
    assert_eq!(name_of(&h, b), "");
    assert!(h.is_scanning(), "the LE scan goes on");

    // the scan ends by itself; the devices stay until the next scan
    h.tick(ultar_btd::stack::host::SCAN_MS + 1);
    let sent = drain(&mut h);
    assert_eq!(
        ops(&sent),
        [op::LE_SET_SCAN_ENABLE],
        "the inquiry already ended"
    );
    assert!(!h.is_scanning());
    assert_eq!(h.devices.len(), 5);
    h.command(Cmd::Scan(true)).unwrap();
    drain(&mut h);
    assert!(h.devices.is_empty(), "a new scan starts with a fresh list");
}

#[test]
fn a_passive_scan_lists_advertisers_without_scan_requests() {
    let mut h = ready(Config {
        le_active: false,
        ..Config::default()
    });
    h.command(Cmd::Scan(true)).unwrap();
    let sent = drain(&mut h);
    let params = sent.iter().find(|s| s.0 == op::LE_SET_SCAN_PARAMS).unwrap();
    assert_eq!(params.1, [0, 0x60, 0, 0x30, 0, 0, 0], "passive");
    h.packet(&adv(0, D, -60, Some((false, "Mou"))));
    let d = Addr {
        bytes: D,
        random: true,
    };
    assert_eq!(
        name_of(&h, d),
        "Mou",
        "the shortened name until a complete one"
    );
}

#[test]
fn a_refused_scan_ends_at_once_and_says_why() {
    let mut h = ready(Config::default());
    h.take_up();
    h.command(Cmd::Scan(true)).unwrap();
    drain_with(&mut h, &[op::LE_SET_SCAN_ENABLE, op::INQUIRY]);
    assert!(!h.is_scanning(), "nothing can be found");
    let up = h.take_up();
    for o in [op::LE_SET_SCAN_ENABLE, op::INQUIRY] {
        assert!(
            up.contains(&Up::Failed {
                opcode: o,
                status: 1
            }),
            "{o:#06x} {up:?}"
        );
    }
    // only one part refused: the other goes on
    h.command(Cmd::Scan(true)).unwrap();
    drain_with(&mut h, &[op::INQUIRY]);
    assert!(h.is_scanning());
    h.packet(&adv(0, D, -60, Some((true, "Mouse"))));
    assert_eq!(h.devices.len(), 1);
}

#[test]
fn a_name_request_that_never_ends_is_given_up() {
    let mut h = ready(Config::default());
    h.command(Cmd::Scan(true)).unwrap();
    drain(&mut h);
    h.packet(&inquiry_rssi(
        &[(A, 0x5a020c, -70), (B, 0x5a020c, -71)],
        false,
    ));
    h.packet(&evt(0x01, &[0]));
    assert_eq!(
        ops(&drain(&mut h))
            .iter()
            .filter(|&&o| o == op::REMOTE_NAME_REQUEST)
            .count(),
        1
    );
    h.tick(ultar_btd::stack::host::NAME_MS + 1);
    let sent = ops(&drain(&mut h));
    assert!(sent.contains(&op::REMOTE_NAME_REQUEST_CANCEL), "{sent:x?}");
    assert!(sent.contains(&op::REMOTE_NAME_REQUEST), "the next one");
}

#[test]
fn a_command_without_an_answer_does_not_hold_the_scan_back() {
    use ultar_btd::stack::host::CMD_MS;
    // a controller that says nothing to Write Secure Connections Host Support: the set-up
    // waits for it, gives it up and goes on
    let silent = op::WRITE_SC_HOST_SUPPORT;
    let mut h = Host::new(Config::default(), Default::default());
    let sent = drain_how(&mut h, |o| (o != silent).then_some(0));
    assert_eq!(ops(&sent).last(), Some(&silent), "waiting for its answer");
    assert!(!h.is_ready());
    h.tick(CMD_MS - 1);
    assert!(drain(&mut h).is_empty(), "not yet given up");
    h.tick(CMD_MS);
    let sent = drain(&mut h);
    assert!(ops(&sent).contains(&op::WRITE_INQUIRY_MODE), "{sent:x?}");
    assert!(h.is_ready(), "set up without it");
    assert!(h.take_up().contains(&Up::Failed {
        opcode: silent,
        status: 0xFF
    }));

    // the scan starts; the late answer to the command given up lets no second command out
    h.command(Cmd::Scan(true)).unwrap();
    let first = h.take_out();
    assert_eq!(first.len(), 1, "one command in flight");
    h.packet(&evt(0x0E, &[1, 0x7A, 0x0C, 0]));
    assert!(h.take_out().is_empty(), "the late answer is not counted");
    let p = &first[0];
    h.packet(&evt(0x0E, &[1, p[1], p[2], 0]));
    let sent = drain(&mut h);
    assert!(ops(&sent).contains(&op::INQUIRY));
    assert!(h.is_scanning());
    h.packet(&adv(0, D, -60, Some((true, "Mouse"))));
    assert_eq!(h.devices.len(), 1);
}

#[test]
fn a_controller_that_cannot_do_both_at_once_scans_in_turns() {
    use ultar_btd::stack::host::{INQUIRY_LENGTH_LATE, SCAN_MS, TURN_MS};
    // Command Disallowed for the inquiry while the LE scan runs
    let mut h = ready(Config::default());
    h.command(Cmd::Scan(true)).unwrap();
    drain_how(&mut h, |o| Some(if o == op::INQUIRY { 0x0C } else { 0 }));
    assert!(h.is_scanning(), "the LE scan goes on");
    h.packet(&adv(0, D, -60, Some((true, "Mouse"))));
    h.tick(TURN_MS - 1);
    assert!(drain(&mut h).is_empty());

    // second turn: LE scan off, then the inquiry alone
    h.tick(TURN_MS);
    let sent = drain(&mut h);
    assert_eq!(ops(&sent), [op::LE_SET_SCAN_ENABLE, op::INQUIRY]);
    assert_eq!(sent[0].1, [0, 0]);
    assert_eq!(sent[1].1, [0x33, 0x8b, 0x9e, INQUIRY_LENGTH_LATE, 0]);
    assert!(u64::from(INQUIRY_LENGTH_LATE) * 1280 <= SCAN_MS - TURN_MS);
    h.packet(&eir(C, 0x002540, -60, "K380"));
    assert_eq!(h.devices.len(), 2, "both kinds found");
    h.packet(&evt(0x01, &[0]));
    assert!(h.is_scanning());
    h.tick(SCAN_MS);
    assert_eq!(
        ops(&drain(&mut h)),
        [op::LE_SET_SCAN_ENABLE],
        "no second turn"
    );
    assert!(!h.is_scanning());
    assert_eq!(h.devices.len(), 2);

    // refused again in its own turn: the scan ends with what the LE scan found
    h.command(Cmd::Scan(true)).unwrap();
    drain_how(&mut h, |o| Some(if o == op::INQUIRY { 0x0C } else { 0 }));
    h.packet(&adv(0, D, -60, Some((true, "Mouse"))));
    h.tick(SCAN_MS + TURN_MS);
    drain_how(&mut h, |o| Some(if o == op::INQUIRY { 0x0C } else { 0 }));
    assert!(!h.is_scanning());
    assert_eq!(h.devices.len(), 1);
}
