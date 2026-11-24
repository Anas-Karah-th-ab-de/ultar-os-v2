//! Classic (BR/EDR) pairing and HID as an HCI trace: Secure Simple Pairing with numeric
//! comparison (the controller does the P-256 work, the host decides), the link key kept,
//! HID control + interrupt channels opened, a boot keyboard report delivered – plus the
//! refusals of everything nobody asked for. Events in Core Vol 4 Part E 7.7 layout.

use ultar_btd::stack::hci::{op, Addr};
use ultar_btd::stack::hid::{InputEvent, InputKind, EV_KEY};
use ultar_btd::stack::host::{Cmd, Config, Host, Up};
use ultar_btd::stack::smp::Ask;

const PEER: [u8; 6] = [0x66, 0x55, 0x44, 0x33, 0x22, 0x11];

fn evt(code: u8, params: &[u8]) -> Vec<u8> {
    [&[0x04, code, params.len() as u8][..], params].concat()
}

fn acl(cid: u16, payload: &[u8]) -> Vec<u8> {
    let mut l2 = (payload.len() as u16).to_le_bytes().to_vec();
    l2.extend_from_slice(&cid.to_le_bytes());
    l2.extend_from_slice(payload);
    let mut p = vec![0x02, 0x0b, 0x20];
    p.extend_from_slice(&(l2.len() as u16).to_le_bytes());
    p.extend_from_slice(&l2);
    p
}

/// Answers every command (complete, status for the slow ones); returns what was sent.
fn drain(h: &mut Host) -> Vec<(u16, Vec<u8>)> {
    let mut sent = Vec::new();
    for _ in 0..100 {
        let out = h.take_out();
        if out.is_empty() {
            break;
        }
        for p in out {
            if p[0] == 0x02 {
                sent.push((0xffff, p[5..].to_vec()));
                h.packet(&evt(0x13, &[1, 0x0b, 0, 1, 0]));
                continue;
            }
            let opcode = u16::from_le_bytes([p[1], p[2]]);
            sent.push((opcode, p[4..].to_vec()));
            let slow = [
                op::INQUIRY,
                op::CREATE_CONNECTION,
                op::AUTH_REQUESTED,
                op::SET_CONN_ENCRYPTION,
                op::DISCONNECT,
            ];
            if slow.contains(&opcode) || opcode == op::REMOTE_NAME_REQUEST {
                h.packet(&evt(0x0F, &[0, 1, p[1], p[2]]));
            } else if opcode == op::READ_BD_ADDR {
                h.packet(&evt(0x0E, &[1, p[1], p[2], 0, 1, 2, 3, 4, 5, 6]));
            } else {
                h.packet(&evt(0x0E, &[1, p[1], p[2], 0]));
            }
        }
    }
    sent
}

fn ops(sent: &[(u16, Vec<u8>)]) -> Vec<u16> {
    sent.iter().map(|s| s.0).collect()
}

fn ready() -> Host {
    let mut h = Host::new(Config::default(), Default::default());
    drain(&mut h);
    assert!(h.is_ready());
    h
}

#[test]
fn a_classic_keyboard_pairs_with_numeric_comparison_and_types() {
    let mut h = ready();
    let peer = Addr::public(PEER);
    h.command(Cmd::Scan(true)).unwrap();
    drain(&mut h);
    let mut eir = [
        &[1u8][..],
        &PEER,
        &[1, 0, 0x40, 0x25, 0x00, 0, 0, 0xc4, 5, 9],
        b"K380",
    ]
    .concat();
    eir.resize(255, 0);
    h.packet(&evt(0x2F, &eir));
    assert_eq!(h.devices[&peer].name, "K380");
    h.command(Cmd::Pair(peer)).unwrap();
    assert!(ops(&drain(&mut h)).contains(&op::CREATE_CONNECTION));
    h.packet(&evt(0x03, &[&[0, 0x0b, 0][..], &PEER, &[1, 0]].concat()));
    assert!(ops(&drain(&mut h)).contains(&op::AUTH_REQUESTED));
    h.packet(&evt(0x17, &PEER));
    assert_eq!(
        drain(&mut h),
        vec![(op::LINK_KEY_NEG_REPLY, PEER.to_vec())],
        "a fresh pairing"
    );
    ssp(&mut h, peer);
    hid_channels(&mut h);
    h.packet(&acl(0x0042, &[0xA1, 1, 0, 0, 4, 0, 0, 0, 0, 0]));
    let input = h.take_up();
    let Some(Up::Input { events, .. }) = input.first() else {
        panic!("{input:?}")
    };
    assert_eq!(
        events[0],
        InputEvent {
            kind: EV_KEY,
            code: 30,
            value: 1
        }
    );
    h.packet(&evt(0x05, &[0, 0x0b, 0, 0x13]));
    assert!(h
        .take_up()
        .iter()
        .any(|u| matches!(u, Up::InputRemoved { .. })));
}

/// IO capabilities, the number compared, the key kept, encryption on.
fn ssp(h: &mut Host, peer: Addr) {
    h.packet(&evt(0x31, &PEER));
    assert_eq!(
        drain(h),
        vec![(op::IO_CAPABILITY_REPLY, [&PEER[..], &[1, 0, 5]].concat())]
    );
    h.packet(&evt(0x32, &[&PEER[..], &[1, 0, 5]].concat()));
    h.take_up();
    h.packet(&evt(0x33, &[&PEER[..], &123_456u32.to_le_bytes()].concat()));
    let token = match h
        .take_up()
        .into_iter()
        .find(|u| matches!(u, Up::Ask { .. }))
    {
        Some(Up::Ask {
            token,
            ask: Ask::Compare(123_456),
            name,
            ..
        }) if name == "K380" => token,
        other => panic!("{other:?}"),
    };
    h.command(Cmd::Answer {
        token,
        accept: true,
    })
    .unwrap();
    assert_eq!(drain(h), vec![(op::USER_CONFIRM_REPLY, PEER.to_vec())]);
    h.packet(&evt(0x36, &[&[0][..], &PEER].concat()));
    h.packet(&evt(0x18, &[&PEER[..], &[7; 16], &[7]].concat()));
    assert!(h.take_up().contains(&Up::BondsChanged));
    assert_eq!(
        h.bonds.find(&peer).map(|b| (b.key, b.authenticated)),
        Some(([7; 16], false))
    );
    h.packet(&evt(0x06, &[0, 0x0b, 0]));
    assert!(ops(&drain(h)).contains(&op::SET_CONN_ENCRYPTION));
    h.packet(&evt(0x08, &[0, 0x0b, 0, 1]));
    assert!(h.take_up().contains(&Up::Paired {
        addr: peer,
        reason: 0
    }));
}

fn hid_channels(h: &mut Host) {
    let sent = drain(h);
    // Connection request for HID control (PSM 0x11) from our channel 0x0041.
    assert!(
        sent.iter()
            .any(|s| s.0 == 0xffff && s.1[4..].starts_with(&[0x02, 1, 4, 0, 0x11, 0, 0x41, 0])),
        "{sent:?}"
    );
    h.packet(&acl(1, &[0x03, 1, 8, 0, 0x50, 0, 0x41, 0, 0, 0, 0, 0]));
    h.packet(&acl(1, &[0x04, 9, 4, 0, 0x41, 0, 0, 0]));
    h.packet(&acl(1, &[0x05, 2, 6, 0, 0x41, 0, 0, 0, 0, 0]));
    let sent = drain(h);
    assert!(
        sent.iter()
            .any(|s| s.1[4..].starts_with(&[0x02, 3, 4, 0, 0x13, 0, 0x42, 0])),
        "interrupt channel"
    );
    h.packet(&acl(1, &[0x03, 3, 8, 0, 0x51, 0, 0x42, 0, 0, 0, 0, 0]));
    h.packet(&acl(1, &[0x04, 10, 4, 0, 0x42, 0, 0, 0]));
    h.packet(&acl(1, &[0x05, 4, 6, 0, 0x42, 0, 0, 0, 0, 0]));
    let sent = drain(h);
    assert!(
        sent.iter().any(|s| s.1 == [1, 0, 0x50, 0, 0x70]),
        "SET_PROTOCOL(boot) on control: {sent:?}"
    );
    let ups = h.take_up();
    assert!(
        ups.iter().any(|u| matches!(
            u,
            Up::InputAdded {
                kind: InputKind::Keyboard,
                ..
            }
        )),
        "{ups:?}"
    );
    // The interrupt channel is our 0x0042: reports arrive there.
    h.packet(&acl(0x0042, &[0xA1, 1, 0, 0, 0, 0, 0, 0, 0, 0]));
    h.take_up();
}

#[test]
fn nothing_pairs_or_connects_unasked() {
    let mut h = ready();
    let stranger = [9, 9, 9, 9, 9, 9];
    h.packet(&evt(0x31, &stranger));
    assert_eq!(
        drain(&mut h),
        vec![(
            op::IO_CAPABILITY_NEG_REPLY,
            [&stranger[..], &[0x18]].concat()
        )]
    );
    h.packet(&evt(0x16, &stranger));
    assert_eq!(
        drain(&mut h),
        vec![(op::PIN_CODE_NEG_REPLY, stranger.to_vec())],
        "no legacy PIN pairing"
    );
    h.packet(&evt(0x33, &[&stranger[..], &[1, 0, 0, 0]].concat()));
    assert_eq!(
        drain(&mut h),
        vec![(op::USER_CONFIRM_NEG_REPLY, stranger.to_vec())]
    );
    h.packet(&evt(0x04, &[&stranger[..], &[0x40, 0x25, 0, 1]].concat()));
    assert_eq!(
        drain(&mut h),
        vec![(op::REJECT_CONNECTION, [&stranger[..], &[0x0F]].concat())]
    );
    h.packet(&evt(0x18, &[&stranger[..], &[1; 16], &[7]].concat()));
    assert!(
        h.bonds.list.is_empty(),
        "a key nobody asked for is not kept"
    );
    assert!(h.take_up().iter().all(|u| !matches!(u, Up::Ask { .. })));
}
